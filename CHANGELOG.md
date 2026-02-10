
# v0.6.0 - 2026-02-04

* Added simple authentication support, write access now requires an api key if `--apikey` is set on startup. Clients must then send `Authorization: Bearer 





# v0.5.2 - 2026-01-20

* Always send a content-type header with text/plain and character encoding (utf-8)





# v0.5.1 - 2026-01-15

This bugfix release fixes a regression that was introduced in 0.5.0 during the implementation of chunked transfer:

*  return chunked data with proper requested range





# v0.5.0 - 2026-01-05

[Miel Peeters]
* Send text as chunked data, this implements support for HTTP/1.1 Chunked Transfer Encoding (https://github.com/knaw-huc/textsurf/pull/6)





# v0.4.2 - 2025-11-14

* improved docker build, much smaller image (thanks to [@JorenSix](https://codeberg.org/JorenSix))
* minor typo correction





# v0.4.1 - 2025-11-10

Updated to textframe 0.3.1





# v0.4.0 - 2025-10-15

This version adds some new entrypoints:

* `GET /path/` entrypoint to get a list (JSON) of files in a particular subdirectory (recursively)
* `DELETE /` entrypoint to delete all resources
* `DELETE /{path}/` entrypoint to delete all resources under a subdirectory
* `PUT /{text_id}` to update *and overwrite* a text (as opposed to `POST` which would return a 403)





# v0.3.0 - 2025-09-15

* This version adds a secondary API (api2) that is inspired on IIIF, see the README for the full specification





# v0.2.1 - 2025-06-06

* Fixed extension handling





# v0.2.0 - 2025-05-20

* defined and implemented a Text Referencing API derived from RFC5147
      * implemented line support (can be disabled with `--no-lines`)
      * implemented validation checks via `length` or `md5`
* implemented directory support, allowing arbitrary nesting to organize files any way the user wants
* make default extension optional to operate in environments with multiple file extensions
* always keep hidden and index files out of the file listing
* bugfix: fix freeze when loading index fails
* bugfix: do not accept all extensions

## Breaking changes

* The API has breaking changes since v0.1.0







# v0.1.1 - 2025-05-19

First release

