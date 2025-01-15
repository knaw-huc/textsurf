[![Project Status: WIP – Initial development is in progress, but there has not yet been a stable, usable release suitable for the public.](https://www.repostatus.org/badges/latest/wip.svg)](https://www.repostatus.org/#wip)
[![Crate](https://img.shields.io/crates/v/textsurf.svg)](https://crates.io/crates/textsurf)
[![Docs](https://docs.rs/textsurf/badge.svg)](https://docs.rs/stamd/)
[![GitHub release](https://img.shields.io/github/release/proycon/textsurf.svg)](https://GitHub.com/proycon/textsurf/releases/)

# Textsurf 

This is a webservice for efficiently serving plain texts and fragments thereof.
It builds upon [textframe](https://github.com/proycon/textframe).

## Description & Features

A RESTful API is offered with several end-points. The full OpenAPI specification can be consulted
interactively at the `/swagger-ui/` endpoint once it is running.

## Web API

The following endpoints are available:

* `GET /`                  - Returns a simple JSON list of all available texts.
* `POST /{text_id}`        - Add a new text 
* `GET /{resource_id}` - Returns a full text given its identifier.
* `GET /{resource_id}/{begin}/{end}` - Returns a text selection inside a resource. Offset are 0-indexed, unicode points, end is non inclusive.
* `GET /swagger-ui`       - Serves an interactive webinterface explaining the RESTful API specification.
* `GET /api-doc/openapi.json`   - Machine parseable OpenAPI specification.

## Installation

### From source

```
$ cargo install textsurf
```

## Usage

Run `textsurf` to start the webservice, see `textsurf --help` for various parameters.

## Security

This webservice is **NOT** meant to be directly opened up to the internet, as
it does not provide any authentication mechanism and can be easily abused as a
file hosting service. It is intended as a backend service for dedicated
frontends to communicate with. Make sure it is behind a firewall or on a
private network segment. If you do expose it to the internet, make sure to
launch stamd with the `--readonly` parameter.
