# Changelog

### v1.0.0 (2022-12-19)
- [Added] Use streaming instead of waiting for forwarding request to reply
- [Fixed] Http error clippy warning

### v0.5.0 (2022-03-18)
- [Added] Make default requests client redirect policy `Policy::none`

### v0.4.0 (2021-10-03)
- [Added] Reqwests client configurable option through OnceCell

### v0.3.2 (2021-06-02)
- [fixed] Missing usage of unique reqwest CLIENT
- [fixed] Suppress breaking API warning until new breaking version is released

### v0.3.1 (2021-02-24)

- [fixed] Unique instance of reqwest client to improve connections
- [changed] Exposed `Error` for compatibility with `warp::Filter::Recover`

### v0.3.0

- [changed] Dependencies update to support `tokio-1` and `warp-0.3`

### Prior

This changelog started on v0.3.0