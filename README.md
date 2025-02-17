[![Project Status: WIP – Initial development is in progress, but there has not yet been a stable, usable release suitable for the public.](https://www.repostatus.org/badges/latest/wip.svg)](https://www.repostatus.org/#wip)
[![Crate](https://img.shields.io/crates/v/textsurf.svg)](https://crates.io/crates/textsurf)
[![Docs](https://docs.rs/textsurf/badge.svg)](https://docs.rs/stamd/)
[![GitHub release](https://img.shields.io/github/release/proycon/textsurf.svg)](https://GitHub.com/proycon/textsurf/releases/)

# Textsurf 

This is a webservice for efficiently serving plain texts and fragments thereof
using unicode character-based addressing. It builds upon
[textframe](https://github.com/proycon/textframe).

## Description & Features

A RESTful API is offered with several end-points. The full OpenAPI specification can be consulted
interactively at the `/swagger-ui/` endpoint once it is running.

The main feature that this service provides is that you can query excerpts of
plain text by unicode character offsets. Internally, there are efficiently
translated to byte offsets and only partially loaded from disk into memory, and
then served.

The service allows upload and deletion of texts, provided this feature is
enabled on startup using the `--writable` flag (make sure you understand the security implications outline further down).

Alternatively, you can consider storing plain text files in a git repository,
cloning that repository on your server (perhaps also periodically pulling
updates via cron), and then serving them immutably using textsurf. Any other
comparable repository or version control system will also do.

Please also see the FAQ section further below.

## Web API

The following endpoints are available:

* `GET /`                  - Returns a simple JSON list of all available texts.
* `GET /{text_id}`         - Returns a full text given its identifier.
* `GET /{text_id}/stat`    - Returns file size and modification date (JSON)
* `GET /{text_id}/{begin}/{end}` - Returns a text selection inside a resource. Offset are 0-indexed, unicode points, end is non inclusive.
* `POST /{text_id}`        - Add a new text
* `DELETE /{text_id}`      - Delete a text
* `GET /swagger-ui`       - Serves an interactive webinterface explaining the RESTful API specification.
* `GET /api-doc/openapi.json`   - Machine parseable OpenAPI specification.

## Installation

### From source

Production environments:

```
$ cargo install textsurf
```

Development environments:

```
$ git clone git@github.com:knaw-huc/textsurf.git
$ cd textsurf
$ cargo install --path .
```

Development versions may require a development version of
[textframe](https://github.com/proycon/textframe) as well, clone it alongside textsurf and add a
`textsurf/.cargo/config.toml` with:

```toml
#[dependencies.textframe]
paths = ["../textframe"]
```

## Usage

Run `textsurf` to start the webservice, see `textsurf --help` for various parameters.

## Security

The webservice launches in read-only mode by default (does not allow text
upload/deletion). Pass `--writable` to allow writing. In that case, the
webservice is **NOT** meant to be directly opened up to the internet, as it
does not provide any authentication mechanism and can be easily abused as a
file hosting service. Make sure it is behind a firewall or on a private network
segment. 

## FAQ

*Q: Can I request byte offsets instead?*

A: No, just use any HTTP/1.1 server that supports the `Range` request header. We
deliberately do not implement this because using byte-offsets may result in malformed unicode responses.

*Q: Will you support other encoding than UTF-8 and other formats than plain text?*

A: No, although for formats with light markup like Markdown or
ReStructuredText, this service may still be useful. For heavy markup like XML
or JSON it is not recommended as character-based addressing makes little sense
there.
