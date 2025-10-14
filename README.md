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
then served. The addressing syntax for the API is derived from [RFC5147](https://www.rfc-editor.org/rfc/rfc5147.txt).

The service allows upload and deletion of texts, provided this feature is
enabled on startup using the `--writable` flag (make sure you understand the security implications outline further down).

Alternatively, you can consider storing plain text files in a git repository,
cloning that repository on your server (perhaps also periodically pulling
updates via cron), and then serving them immutably using textsurf. Any other
comparable repository or version control system will also do.

Please also see the FAQ section further below.

## Text Referencing API: Endpoints

The following endpoints are defined and consistute the *Text Referencing API*, which will be more formally defined in a later section:

* `GET /`                  - Returns a simple JSON list of all available texts (recursively)
* `GET /{text_id}`         - Returns a full text given its identifier.
* `GET /{text_id}?char={begin},{end}` - Returns a text selection inside a resource. Offset are 0-indexed, unicode points, end is non inclusive. This implements part of [RFC5147](https://www.rfc-editor.org/rfc/rfc5147.txt) server-side.
* `GET /{text_id}?line={begin},{end}` - Returns a text selection inside a resource by line range. Offset are 0-indexed lines (so the first line is 0 and not 1!), end is non inclusive. This implements another part of [RFC5147](https://www.rfc-editor.org/rfc/rfc5147.txt) server-side.
* `GET /{path}/`            - Returns a simple JSON list of all available texts under this path (recursively). The trailing slash is key here.
* `DELETE /{text_id}`      - Delete a text
* `POST /{text_id}`        - Add a new text
* `GET /stat/{text_id}`    - Returns file size and modification date (JSON)

In all these instances `text_id` may itself consist of any number of path
components, a filename, and optionally an extension. If no explicit extension
is provided, the server may use an implied a default one (usually `.txt`).
Allowing a full path allows you to use arbitrary hierarchies to organize text files. 

### Extra endpoints

These are extra endpoints that are available but not part of the Text Referencing API:

* `GET /{text_id}?begin={begin}&end={end}` - Returns a text selection inside a resource. Offset are 0-indexed, unicode points, end is non inclusive. Alternative syntax similar to the above.
* `GET /s/{text_id}/{begin}/{end}` - Simple pure URL call. Only works with simple text IDs without any path components!
* `GET /swagger-ui`        - Serves an interactive webinterface explaining the RESTful API specification.
* `GET /api-doc/openapi.json`   - Machine parseable OpenAPI specification.


## Text Referencing API 1: Formal Specification

Textsurf implements a minimal **Text Referencing API** that is directly derived from 
[RFC5147](https://www.rfc-editor.org/rfc/rfc5147.txt). RFC5147 specifies URI
*fragment identifiers* for the `text/plain` media type, in the form of, e.g:
`https://example.org/test.txt#char=10,20`. It is a *fragment specification* and
therefore applies to the client-side, not the server side. Textsurf, however, is a server. 
We take this RFC5417 spec and turn it into an API.

The capitalized key words "MUST", "MUST NOT", "REQUIRED", "SHALL",
"SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and
"OPTIONAL" in this section are to be interpreted as described in 
[RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

This Text Referencing API lays down the following constraints:

1. A text file *MUST BE* unambiguous identified by a URI (as defined by [RFC3986](https://www.rfc-editor.org/rfc/rfc3986)). 
A URI to a text file as a whole *SHOULD NOT* have a query part (starting with `?`). Example: `https://example.org/test.txt`
    1. The text file *MUST BE* retrievable by its full extension. It *MAY* also be retrievable by having an implied default extension. Example: `https://example.org/test`
    2. There *SHOULD NOT* be a trailing slash.
    3. The URI *SHOULD* support an arbitrary number of path components after the base components where the server resides, allowing a full directory hierarchy to organize text files. Example: `https://example.org/deep/in/the/forest/test.txt`
2. A text file *MUST* be retrievable via a `HTTP GET` call on its URI.
    1. A text file *MUST* be served with media-type `text/plain` with character encoding `UTF-8` and UNIX line endings. (linefeed,  `0x0a`, `\n`)
3. A text file *MUST* be submittable via a `HTTP POST` call on its URI, provided the server is not in a read-only state.
    1. If the text file contains path components, the necessary directories *SHOULD* be automatically created.
    2. The file is transferred in the request body.
4. A text file *MUST* be removable via a `HTTP DELETE` call on its URI, provided the server is not in a read-only state.
5. A text excerpt inside a text file is identified by using the fragment identifier syntax 
as defined in section 3 of [RFC5147](https://www.rfc-editor.org/rfc/rfc5147.txt) in the query part (starting with `?`) of its URI, rather than in the  fragment part (starting with `#`). Examples: `https://example.org/test.txt?char=10,20` ,  `https://example.org/test.txt?line=0,1` ,  `https://example.org/test.txt?line=0,1&length=104&md5=b07ec26b0c68933887b28278becdc5f9`
    * A text excerpt is defined as a single contingent subpart of the whole text where the begin and endpoints are defined.
    * This means that clients implementing RFC5147 can effectively shift the burden of implementation to a server by moving the fragment part to the query part. (i.e. replacing `#` with `?`).
7. An endpoint `/stat/{text_id}` *SHOULD* be provided that provides at least the following information as keys in a JSON response:
    * `bytes` - The filesize of the file in bytes
    * `chars` - The length of the text file in unicode points.
    * `checksum` - A SHA-256 checksum of the entire textfile.
    * `mtime` - The modification time of the file in number of seconds since the unix epoch (1970-01-01 00:00).
6. Any of the endpoints *MAY* be restricted to authenticated or authorized users only., This specification does not define a specific mechanism for that as it is beyond it's scope.

## Text Referencing API 2: Formal Specification

In addition to the above API, Textsurf implements a **second Text
Referencing API**. Though there are two separate interfaces, the functionality they
expose is identical and it is a matter of preference which one you want to use.
The secondary API is available under the `/api2/` endpoint. It was designed not
to use query parameters, interoperate closer with linked open data, and is
modelled after the [IIIF Image API](https://iiif.io/api/image/3.0/). 

The capitalized key words "MUST", "MUST NOT", "REQUIRED", "SHALL",
"SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and
"OPTIONAL" in this section are to be interpreted as described in 
[RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

1. A request for a text file or text segment therein *MUST* be a `HTTP GET` request conforming to the following URI template: `{scheme}://{server}{/prefix}/{identifier}{/region}`
    * `scheme` - Indicates the use of the HTTP or HTTPS protocol in calling the service.
    * `server` - The host server on which the service resides. The parameters *MAY* also contain a port number.
    * `prefix` - The path on the host server to the service. This prefix is *OPTIONAL* from the point of view of this specification, but it is *REQUIRED* to end in `/api2` for the TextSurf implementation. A prefix may be useful when the host server supports multiple services. The prefix may contain multiple path segments, delimited by slashes, but all other special characters must be encoded.
    * `identifier` - The identifier of the requested text. This must be a filename and *MAY* contain path information, but special characters including slashes for directory hierarchy *MUST* be URI encoded. The text file *MUST* be retrievable by its full extension. It *MAY* also be retrievable by having an implied default extension. Example: `https://example.org/api2/test` for `https://example.org/api2/test.txt`
    * `region` - This parameter is *OPTIONAL* and used when requesting a subpart of the text. Syntax is as follows:
        * `full` - Returns the full text, same as just omitted the region parameter entirely
        * `{begin},{end}` - Returns the text from character begin to end. 
            * Characters correspond to unicode points and *MUST* be 0-indexed, the end *MUST* be non-inclusive.  Example: `0,1` returns the first character of a text. 
            * Negative offsets *MUST* be supported and are interpreted relative to the end of the text. 
            * If the end value is omitted, the offset is interpreted to be the end of the text. Example: `-1,` returns the last character of a text.
        * `char:{begin},{end}` - Same as above
        * `line:{begin},{end}` - Returns lines, lines *MUST* be 0-indexed and the end *MUST* be non-inclusive.
2. A text file *MUST* be submittable via a `HTTP POST` call on the same URI as in point 1, but without the region part, and provided the server is not in a read-only state.
    1. If the text file contains path components, the necessary directories *SHOULD* be automatically created.
    2. The file is transferred in the request body.
3. A text file *MUST* be removable via a `HTTP DELETE` call on its URI, provided the server is not in a read-only state.
4. A request for text information *MUST* conform to the following URI template: `{scheme}://{server}{/prefix}/{identifier}/info.json`. This *SHOULD* return a *JSON* response with the following keys:
    * `@context` - `https://w3id.org/textsurf/api2.jsonld`
    * `id` - URI of the text file
    * `type` - `TextService2`
    * `protocol` - `https://w3id.org/textsurf/api2`
    * `bytes` - The filesize of the file in bytes
    * `chars` - The length of the text file in unicode points.
    * `checksum` - A SHA-256 checksum of the entire textfile.
    * `mtime` - The modification time of the file in number of seconds since the unix epoch (1970-01-01 00:00).
5. Any of the endpoints *MAY* be restricted to authenticated or authorized users only., This specification does not define a specific mechanism for that as it is beyond it's scope.

## Installation

You can install textsurf as follows:

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
