use crate::common::{ApiError, ApiResponse};
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use textframe::TextFile;
use tracing::info;

const WAIT_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub struct State {
    last_access: Duration,
    loading: bool,
}

pub struct TextPool {
    basedir: PathBuf,
    extension: String,
    readonly: bool,
    unload_time: u64,
    texts: RwLock<HashMap<String, Arc<RwLock<TextFile>>>>, //the extra Arc allows us to drop the lock earlier
    states: RwLock<HashMap<String, State>>,
}

impl TextPool {
    pub fn new(
        basedir: impl Into<PathBuf>,
        extension: impl Into<String>,
        readonly: bool,
        unload_time: u64,
    ) -> Result<Self, &'static str> {
        let basedir: PathBuf = basedir.into();
        if !basedir.is_dir() {
            Err("Base directory must exist")
        } else {
            Ok(Self {
                basedir,
                extension: extension.into(),
                texts: HashMap::new().into(),
                states: HashMap::new().into(),
                unload_time,
                readonly,
            })
        }
    }

    pub fn basedir(&self) -> &Path {
        self.basedir.as_path()
    }

    pub fn extension(&self) -> &str {
        self.extension.as_str()
    }

    pub fn map<F, T>(&self, id: &str, begin: isize, end: isize, f: F) -> Result<T, ApiError>
    where
        F: FnOnce(&str) -> Result<T, ApiError>,
    {
        let _state = self.load(id)?;
        if let Ok(texts) = self.texts.read() {
            if let Some(textlock) = texts.get(id).cloned() {
                drop(texts); //compiler should be able to infer this but better safe than sorry
                if let Ok(mut textfile) = textlock.write() {
                    //we need a write lock because we may load a new part of the text from disk here
                    let text = textfile.get_or_load(begin, end)?; //this triggers a load from disk of a part of the text unless it's already covered by a part that was loaded earlier
                    f(&text)
                } else {
                    Err(ApiError::InternalError("Textfiles lock got poisoned")) //only happens if a thread holding a write lock panics
                }
            } else {
                unreachable!("text file should have been  loaded in first line")
            }
        } else {
            Err(ApiError::InternalError("Lock poisoned: textfiles"))
        }
    }

    pub fn stat(&self, id: &str) -> Result<ApiResponse, ApiError> {
        let _state = self.load(id)?;
        if let Ok(texts) = self.texts.read() {
            if let Some(textlock) = texts.get(id).cloned() {
                drop(texts); //compiler should be able to infer this but better safe than sorry
                if let Ok(textfile) = textlock.read() {
                    Ok(ApiResponse::Stat {
                        chars: textfile.len() as u64,
                        bytes: textfile.len_utf8() as u64,
                        mtime: textfile.mtime(),
                        checksum: textfile.checksum_digest(),
                    })
                } else {
                    Err(ApiError::InternalError("Textfiles lock got poisoned")) //only happens if a thread holding a write lock panics
                }
            } else {
                unreachable!("text file should have been  loaded in first line")
            }
        } else {
            Err(ApiError::InternalError("Lock poisoned: textfiles"))
        }
    }

    /// Create a new text
    pub fn new_text(&self, id: &str, text: String) -> Result<(), ApiError> {
        if self.readonly {
            return Err(ApiError::PermissionDenied("Service is readonly"));
        }
        let filename = self.filename_from_id(id)?;
        if filename.exists() {
            Err(ApiError::PermissionDenied("Text already exists"))
        } else {
            info!("Creating {}", id);
            let mut file = File::create(filename)?;
            file.write(text.as_bytes())?;
            Ok(())
        }
    }

    /// Loads a text resource into the pool
    /// Note that this loads/computes the index, not any actual text yet
    /// Only one thread can load at a time.
    /// Returns a **copy** of the state
    fn load(&self, id: &str) -> Result<State, ApiError> {
        let mut loading: Option<bool> = None;

        //loop in case we have to wait for another thread to do the loading
        loop {
            if let Ok(states) = self.states.read() {
                if let Some(state) = states.get(id) {
                    loading = Some(state.loading);
                }
            } else {
                return Err(ApiError::InternalError("Lock poisoned"));
            }
            match loading {
                Some(true) => {
                    //already loading in another thread
                    std::thread::sleep(WAIT_INTERVAL);
                }
                Some(false) => {
                    //already loaded, we update the access time only
                    if let Ok(mut states) = self.states.write() {
                        if let Some(state) = states.get_mut(id) {
                            state.last_access =
                                SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                            return Ok(state.clone());
                        } else {
                            return Err(ApiError::InternalError("State must exist"));
                        }
                    } else {
                        return Err(ApiError::InternalError("Lock poisoned"));
                    }
                }
                None => break, //not loaded yet
            }
        }
        let filename = self.filename_from_id(id)?;
        if !filename.exists() {
            return Err(ApiError::NotFound("No such text exists"));
        }

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        if let Ok(mut states) = self.states.write() {
            //mark as loading
            states.insert(
                id.to_string(),
                State {
                    last_access: now,
                    loading: true,
                },
            );
        } else {
            return Err(ApiError::InternalError("Lock poisoned"));
        }

        //loading/indexing (potentially time intensive) done here is done without any locks held
        //it loads/computes only the index, not the full text.
        info!("Loading {}", id);
        let indexname = filename.with_extension("index"); //cached index
        match TextFile::new(filename, Some(&indexname)) {
            Ok(textfile) => {
                if let Ok(mut texts) = self.texts.write() {
                    texts.insert(id.to_string(), Arc::new(RwLock::new(textfile)));
                } else {
                    return Err(ApiError::InternalError("Lock poisoned"));
                }
            }
            Err(e) => {
                return Err(ApiError::TextError(e));
            }
        }

        //mark loading as done:
        if let Ok(mut states) = self.states.write() {
            if let Some(state) = states.get_mut(id) {
                state.loading = false;
                Ok(state.clone())
            } else {
                return Err(ApiError::InternalError("State must exist"));
            }
        } else {
            return Err(ApiError::InternalError("Lock poisoned"));
        }
    }

    /// Gets the filename from the ID, validating the ID in the process
    fn filename_from_id(&self, id: &str) -> Result<PathBuf, ApiError> {
        //some security checks so the user can't break out of the configured base directory
        let basename: PathBuf = self.check_basename(id)?;

        Ok(self
            .basedir
            .clone()
            .join(basename.clone())
            .with_extension(&self.extension))
    }

    fn wait_until_ready(&self, id: &str) -> Result<State, ApiError> {
        //loop in case we have to wait for another thread to do loading or saving
        let mut wait = false;
        loop {
            if let Ok(states) = self.states.read() {
                if let Some(state) = states.get(id) {
                    wait = state.loading;
                    if !wait {
                        return Ok(state.clone());
                    }
                }
            } else {
                return Err(ApiError::InternalError("Lock poisoned"));
            }
            if wait {
                std::thread::sleep(WAIT_INTERVAL);
            } else {
                return Err(ApiError::NotFound("No such text loaded"));
            }
        }
    }

    /// Unload a text from the pool if it is loaded (no-op if it isn't loaded)
    pub fn unload(&self, id: &str) -> Result<(), ApiError> {
        match self.wait_until_ready(id) {
            Ok(_) => {
                if let Ok(mut texts) = self.texts.write() {
                    if texts.contains_key(id) {
                        texts.remove(id);
                    }
                } else {
                    return Err(ApiError::InternalError("Lock poisoned"));
                }

                if let Ok(mut states) = self.states.write() {
                    if states.contains_key(id) {
                        states.remove(id);
                    }
                } else {
                    return Err(ApiError::InternalError("Lock poisoned"));
                }

                info!("Unloaded {}", id);
                Ok(())
            }
            Err(ApiError::NotFound(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn flush(&self, force: bool) -> Result<Vec<String>, ApiError> {
        let mut remove_ids: Vec<String> = Vec::new();

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

        if let Ok(states) = self.states.read() {
            for (id, state) in states.iter() {
                if force || (now - state.last_access).as_secs() >= self.unload_time {
                    remove_ids.push(id.to_string());
                }
            }
        } else {
            return Err(ApiError::InternalError("Lock poisoned"));
        }

        for id in remove_ids.iter() {
            self.unload(&id)?;
        }

        Ok(remove_ids)
    }

    fn check_basename(&self, id: &str) -> Result<PathBuf, ApiError> {
        let filename: PathBuf = id.into();

        //some security checks so the user can't break out of the configured base directory
        if filename.is_absolute() {
            return Err(ApiError::NotFound(
                "No such annotationstore exists (no absolute paths allowed)",
            ));
        }
        for (i, component) in filename.components().enumerate() {
            if i > 0 {
                return Err(ApiError::NotFound("Filename may not contain a directory"));
            }
            if component == Component::ParentDir {
                return Err(ApiError::NotFound(
                    "No such text exists (no parent directories allowed)",
                ));
            }
        }
        Ok(filename)
    }

    pub fn delete_text(&self, text_id: &str) -> Result<(), ApiError> {
        if self.readonly {
            return Err(ApiError::PermissionDenied("Service is readonly"));
        }
        let filename = self.filename_from_id(text_id)?;
        if filename.exists() {
            self.unload(text_id)?;
            let cachefilename = filename.with_extension("index");
            std::fs::remove_file(filename)?;
            //also remove index file:
            if cachefilename.exists() {
                std::fs::remove_file(cachefilename)?;
            }
            Ok(())
        } else {
            Err(ApiError::NotFound("No such text"))
        }
    }
}

impl Drop for TextPool {
    fn drop(&mut self) {
        if !self.readonly {
            self.flush(true).expect("Clean shutdown failed");
        }
    }
}
