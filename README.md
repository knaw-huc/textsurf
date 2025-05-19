[![Project Status: Active – The project has reached a stable, usable state and is being actively developed.](https://www.repostatus.org/badges/latest/active.svg)](https://www.repostatus.org/#active)
[![Crate](https://img.shields.io/crates/v/textsurf.svg)](https://crates.io/crates/textsurf)
[![GitHub release](https://img.shields.io/github/release/proycon/textsurf.svg)](https://GitHub.com/proycon/textsurf/releases/)

# Textsurf 

<p align="center">
    <img src="https://github.com/knaw-huc/textsurf/raw/master/logo.png" alt="textsurf logo" width="320" />
</p>

This is a webservice for efficiently serving plain texts and fragments thereof
using unicode character-based addressing. It builds upon
[textframe](https://github.com/proycon/textframe).

## Description & Features

A RESTful API is offered with several end-points. The full OpenAPI specification can be consulted
interactively at the `/swagger-ui/` endpoint once it is running.

The main feature that this service provides is that you can query excerpts of
plain text by unicode character offsets. Internally, they are efficiently
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
* `GET /{text_id}?char={begin},{end}` - Returns a text selection inside a resource. Offset are 0-indexed, unicode points, end is non inclusive. This implements part of [RFC5147](https://www.rfc-editor.org/rfc/rfc5147.txt) server-side.
* `GET /{text_id}?begin={begin}&end={end}` - Returns a text selection inside a resource. Offset are 0-indexed, unicode points, end is non inclusive. Alternative syntax.
* `GET /s/{text_id}/{begin}/{end}` - Simple pure URL call. Only works with simple text IDs (see note at the end).
* `POST /{text_id}`        - Add a new text
* `DELETE /{text_id}`      - Delete a text
* `GET /stat/{text_id}`    - Returns file size and modification date (JSON)
* `GET /swagger-ui`        - Serves an interactive webinterface explaining the RESTful API specification.
* `GET /api-doc/openapi.json`   - Machine parseable OpenAPI specification.

In all these instances except for the `/s/` endpoint, `text_id` may itself consist of a path. Only file extension (`.txt` by default) is not included. This allows arbitrary hierarchies to organize text files. 

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
[textframe](https://github.com/proycon/textframe); clone it alongside textsurf and add a
`textsurf/.cargo/config.toml` with:

```toml
#[dependencies.textframe]
paths = ["../textframe"]
```

### As a container

Run ``make docker`` to build a container using docker or podman.

## Usage

Run `textsurf` to start the webservice, see `textsurf --help` for various parameters.

### Container usage

Run `docker run --rm -v ./test/docroot:/data -p 8080:8080 proycon/textsurf` where `./test/docroot/` is the document root path containing text files that you want to mount into the container. The service will be available on `127.0.0.1:8080`. Make sure that subuid 1000 inside the container is mapped to a user on the host that has read and write access to the files. You can pass `--env DEBUG=1` for more verbose output.

## Security

The webservice launches in read-only mode by default (does not allow text
upload/deletion). Pass `--writable` to allow writing (for the container, pass environment variable `WRITABLE=1`). 
In that case, the webservice is **NOT** meant to be directly opened up to the internet, as it
does not provide any authentication mechanism and can be easily abused as a
an arbitrary file hosting service. Make sure it is behind a firewall or on a private network
segment. 

## FAQ

*Q: Can I request byte offsets instead?*

A: No, just use any HTTP/1.1 server that supports the `Range` request header. We
deliberately do not implement this because using byte-offsets may result in malformed unicode responses.

*Q: Will you support other encodings than UTF-8 and other formats than plain text?*

A: No, although for formats with light markup like Markdown or
ReStructuredText, this service may still be useful. For heavy markup like XML
or JSON it is not recommended as character-based addressing makes little sense
there.

*Q: How does this relate to RFC5147?*

[RFC5147](https://datatracker.ietf.org/doc/html/rfc5147) specifies URI fragment identifiers for text/plain media type, in the form of, e.g: `https://example.org/test.txt#char=10,20`. It is a *fragment specification* and therefore applies to the client-side, not the server side. Textsurf, on the other hand, is a server. Clients who want to implement textsurf support can translate RFC5147 compliant URIs to textsurf API calls, which is modelled after the same specification. This effectively shifts the burden to the server instead of the client and letting textsurf do the job of returning the fragment.
