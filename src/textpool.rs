use crate::common::{ApiError, ApiResponse};
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use textframe::{TextFile, TextFileMode};
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
    lines: bool,
    unload_time: u64,
    texts: RwLock<HashMap<String, Arc<RwLock<TextFile>>>>, //the extra Arc allows us to drop the lock earlier
    states: RwLock<HashMap<String, State>>,
}

impl TextPool {
    pub fn new(
        basedir: impl Into<PathBuf>,
        extension: impl Into<String>,
        readonly: bool,
        lines: bool,
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
                lines,
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
                    f(text)
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

    pub fn map_lines<F, T>(&self, id: &str, begin: isize, end: isize, f: F) -> Result<T, ApiError>
    where
        F: FnOnce(&str) -> Result<T, ApiError>,
    {
        let _state = self.load(id)?;
        if let Ok(texts) = self.texts.read() {
            if let Some(textlock) = texts.get(id).cloned() {
                drop(texts); //compiler should be able to infer this but better safe than sorry
                if let Ok(mut textfile) = textlock.write() {
                    //we need a write lock because we may load a new part of the text from disk here
                    let text = textfile.get_or_load_lines(begin, end)?; //this triggers a load from disk of a part of the text unless it's already covered by a part that was loaded earlier
                    f(text)
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

    pub fn stat_api2(&self, id: &str) -> Result<ApiResponse, ApiError> {
        let _state = self.load(id)?;
        if let Ok(texts) = self.texts.read() {
            if let Some(textlock) = texts.get(id).cloned() {
                drop(texts); //compiler should be able to infer this but better safe than sorry
                if let Ok(textfile) = textlock.read() {
                    Ok(ApiResponse::StatLD {
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

    /// Create a new text. Returns true if it was newly created
    pub fn new_text(&self, id: &str, text: String, overwrite: bool) -> Result<bool, ApiError> {
        if self.readonly {
            return Err(ApiError::PermissionDenied("Service is readonly"));
        }
        let filename = self.filename_from_id(id)?; //this also does validation and security checks
        let exists = filename.exists();
        if exists && !overwrite {
            Err(ApiError::PermissionDenied("Text already exists"))
        } else {
            info!("Creating {}", id);
            if let Some(parentdir) = filename.parent() {
                std::fs::create_dir_all(parentdir)?;
            }
            let mut file = File::create(filename)?;
            let _ = file.write(text.as_bytes())?;
            Ok(!exists)
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
        let mode = if self.lines {
            TextFileMode::WithLineIndex
        } else {
            TextFileMode::NoLineIndex
        };
        match TextFile::new(filename, Some(&indexname), mode) {
            Ok(textfile) => {
                if let Ok(mut texts) = self.texts.write() {
                    texts.insert(id.to_string(), Arc::new(RwLock::new(textfile)));
                } else {
                    if let Ok(mut states) = self.states.write() {
                        states.remove(id);
                    }
                    return Err(ApiError::InternalError("Lock poisoned"));
                }
            }
            Err(e) => {
                if let Ok(mut states) = self.states.write() {
                    states.remove(id);
                }
                return Err(ApiError::TextError(e));
            }
        }

        //mark loading as done:
        if let Ok(mut states) = self.states.write() {
            if let Some(state) = states.get_mut(id) {
                state.loading = false;
                Ok(state.clone())
            } else {
                Err(ApiError::InternalError("State must exist"))
            }
        } else {
            Err(ApiError::InternalError("Lock poisoned"))
        }
    }

    /// Gets the filename from the ID, validating the ID in the process
    fn filename_from_id(&self, id: &str) -> Result<PathBuf, ApiError> {
        //some security checks so the user can't break out of the configured base directory
        let basename: PathBuf = self.check_basename(id)?;
        let mut filename = self.basedir.clone().join(basename.clone());
        if !self.extension.is_empty() {
            if filename.extension().is_none() {
                filename = filename.with_extension(&self.extension);
            } else if filename.extension().unwrap() != self.extension.as_str() {
                //add extension
                filename = filename.with_file_name(format!(
                    "{}.{}",
                    filename.file_name().unwrap().to_string_lossy(),
                    self.extension
                ));
            }
        } else if filename.extension().map(|x| x.as_encoded_bytes()) == Some(b"index") {
            return Err(ApiError::NotFound("An index is not a valid text"));
        }
        if filename
            .file_name()
            .map(|x| x.as_encoded_bytes().first() == Some(&46)) //ASCII for .  (might break on other OSes that use something like UTF-16
            .unwrap_or(false)
        {
            //hidden files are never served
            Err(ApiError::NotFound("No such file"))
        } else {
            Ok(filename)
        }
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
            self.unload(id)?;
        }

        Ok(remove_ids)
    }

    fn check_basename(&self, id: &str) -> Result<PathBuf, ApiError> {
        let filename: PathBuf = id.into();

        //some security checks so the user can't break out of the configured base directory
        if filename.is_absolute() {
            return Err(ApiError::NotFound(
                "No such text exists (no absolute paths allowed)",
            ));
        }
        for component in filename.components() {
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

    pub fn absolute_pos(
        &self,
        id: &str,
        begin: isize,
        end: isize,
    ) -> Result<(usize, usize), ApiError> {
        let _state = self.load(id)?;
        if let Ok(texts) = self.texts.read() {
            if let Some(textlock) = texts.get(id).cloned() {
                drop(texts); //compiler should be able to infer this but better safe than sorry
                if let Ok(textfile) = textlock.read() {
                    textfile
                        .absolute_pos(begin, end)
                        .map_err(ApiError::TextError)
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

    /// Convert relative line range to absolute character range
    pub fn absolute_line_pos(
        &self,
        id: &str,
        begin: isize,
        end: isize,
    ) -> Result<(usize, usize), ApiError> {
        let _state = self.load(id)?;
        if let Ok(texts) = self.texts.read() {
            if let Some(textlock) = texts.get(id).cloned() {
                drop(texts); //compiler should be able to infer this but better safe than sorry
                if let Ok(textfile) = textlock.read() {
                    textfile
                        .absolute_line_pos(begin, end)
                        .map_err(ApiError::TextError)
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
}

impl Drop for TextPool {
    fn drop(&mut self) {
        if !self.readonly {
            self.flush(true).expect("Clean shutdown failed");
        }
    }
}
