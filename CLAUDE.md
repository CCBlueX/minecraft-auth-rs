# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
cargo build                          # build the library
cargo test                           # run all tests (all tests are inline `#[cfg(test)] mod tests` per file)
cargo test <test_name>               # run a single test by name (substring match)
cargo test <module>::                # run all tests in one module, e.g. `cargo test xbl::sign::`
cargo fmt                            # format
cargo fmt --check                    # verify formatting without writing
cargo clippy --all-targets -- -D warnings  # lint, denying warnings
```

There is no separate `tests/` directory and no CI config in this repo yet — tests live next to the code they cover.

## Style

Keep prose short — in commits and in comments alike, less is usually better.

- **Commits:** a brief line saying what it fixes. No paragraphs of rationale or background. Do not add a `Claude-Session:` trailer.
- **Comments:** explain what the code can't say for itself — a protocol quirk, a non-obvious constraint. Skip the rest.

## Architecture

This crate implements the Java Edition Microsoft → Xbox Live (SISU) → Minecraft Services login chain as three sequential modules, each following the same internal shape (`config.rs`/`model.rs`/`request.rs` plus a module-specific entry point):

```
src/msa/   MSA (Microsoft Account) device-code / webview OAuth login and token refresh
src/xbl/   Xbox Live device token + SISU authorize, including request signing
src/java/  Minecraft Services login, entitlements, profile, player certificates
```

`JavaAuthManager` (`src/java/manager.rs`) is the orchestrator: it chains msa → xbl → java, and each stage is a `Holder<T>` (`src/expirable.rs`) — a lazily-refreshed, expiry-aware cache. Calling e.g. `.profile()` walks the chain, refreshing only the stages that are missing or expired, and callers converge on a single in-flight refresh instead of firing duplicate requests. `JavaAuthManager::to_json`/`from_json` (de)serialize the whole cached chain for persistence between runs; fields not yet fetched are simply omitted.

Cross-cutting pieces used by more than one stage:
- `src/crypto.rs` — `EcKeyPair`, a P-256 key pair used for Xbox Live proof-of-possession signing (IEEE P1363 signatures), serializable via PKCS8 DER.
- `src/clock.rs` — samples Microsoft's server clock offset once (from the `Date` header of a `login.live.com` request) and caches it in a `OnceCell`, since Xbox Live rejects signed requests with too much clock drift.
- `src/xbl/sign.rs` — builds the XBL `Signature` header: a Windows-epoch timestamp plus an ECDSA signature over a null-byte-delimited buffer of (timestamp, method, path, `Authorization` header, body).
- `src/error.rs` — single crate-wide `Error` enum (`thiserror`). Variants intentionally never carry access tokens, refresh tokens, authorization codes, or private keys.

Design constraints that shape the API:
- The library owns no global runtime, browser, or storage location. Callers supply their own `reqwest::Client` to `JavaAuthManager::builder(..)`.
- The webview login flow (`login_webview`) takes a plain `FnOnce(Url) -> impl Future<Output = Result<Url>>` closure — the caller drives an embedded browser/webview and hands back the final redirect URL. It is not a trait requiring a browser dependency.
- Device-code login (`login_device_code`) takes an `on_code` callback invoked once with the code/verification URL, then polls internally until the user completes sign-in or it times out.

Scope: only the Java Edition SISU login path is implemented (no Bedrock, PlayFab, Realms, Xbox profile/gamertag lookup, or the legacy non-title 3-leg XBL auth path). See `README.md`'s "Upstream" section for the current baseline commit.

## Staying in sync with upstream

Wire behavior (request shapes, headers, field names, signing, error codes) is checked against [RaphiMC/MinecraftAuth](https://github.com/RaphiMC/MinecraftAuth) — there's no shared build tooling between the two projects, so syncing is a manual, periodic review against its Java source, treated as a protocol spec rather than something to translate line by line.

**When to re-check:** before a release, when Microsoft/Xbox/Mojang auth breaks in the wild (expired-refresh loops, new required fields, XSTS errors that don't map to anything in `src/xbl/error.rs`), or when upstream tags a new release.

**How to re-check:** clone upstream and diff from the commit named in `README.md`'s "Upstream" section:

```sh
git clone https://github.com/RaphiMC/MinecraftAuth.git /tmp/minecraftauth-upstream
cd /tmp/minecraftauth-upstream
git log --oneline <baseline-commit>..HEAD -- src/main/java/net/raphimc/minecraftauth
```

An empty log means nothing changed in scope. Otherwise, for each changed file, find its counterpart below, compare request/response shape and field names, and update `README.md`'s baseline commit once done.

**File mapping** (upstream Java → this crate):

| Concern | Upstream (Java) | This crate (Rust) |
|---|---|---|
| MSA endpoints, title IDs, scopes | `msa/data/MsaConstants.java`, `msa/data/MsaEnvironment.java` | `src/msa/config.rs` (`constants` module, `MsaEnvironment`) |
| MSA application config / authorize URL | `msa/model/MsaApplicationConfig.java` | `src/msa/config.rs` (`MsaApplicationConfig`) |
| MSA token/device-code requests | `msa/request/*.java` | `src/msa/request.rs` |
| MSA device-code polling loop | `msa/service/impl/DeviceCodeMsaAuthService.java` | `src/msa/transport.rs::login_with_device_code_timeout` |
| MSA browser/webview redirect flow | `msa/service/impl/LocalWebServerMsaAuthService.java`, `JfxWebViewMsaAuthService.java` | `src/msa/transport.rs::login_with_webview` |
| XBL relying parties | `xbl/data/XblConstants.java` | `src/xbl/mod.rs` (`constants` module) |
| XBL device/SISU request shape | `xbl/request/XblDeviceAuthenticateRequest.java`, `XblSisuAuthorizeRequest.java` | `src/xbl/request.rs` |
| XBL request signing (proof key, `Signature` header) | `xbl/request/SignedXblPostRequest.java`, `util/CryptUtil.java`, `util/TimeUtil.java` | `src/xbl/sign.rs`, `src/crypto.rs`, `src/clock.rs` |
| XBL error codes | `xbl/exception/XblRequestException.java` | `src/xbl/error.rs` |
| Java Services requests (login, entitlements, profile, certificates) | `java/request/*.java` | `src/java/request.rs` |
| Java pipeline / lazy refresh orchestration | `java/JavaAuthManager.java`, `util/holder/Holder.java` | `src/java/manager.rs`, `src/expirable.rs` |
| Expiry semantics | `util/Expirable.java` | `src/expirable.rs::Expirable` |

Rust model struct fields (`src/msa/model.rs`, `src/xbl/model.rs`, `src/java/model.rs`) intentionally use `snake_case` for this crate's own persisted JSON shape — they don't need to byte-for-byte match the Java `toJson`/`fromJson` output, since save-state compatibility between the two libraries is out of scope. What must match byte-for-byte is the **wire** shape: the JSON sent to and parsed from Microsoft/Xbox/Mojang's own APIs (the `#[serde(rename = "...")]` fields and request bodies built with `serde_json::json!`).

**Adding back a deferred area** (Bedrock, Realms, PlayFab, Xbox profile):

1. Read the matching upstream package (`bedrock/`, `extra/realms/`, `playfab/`) end to end first — these packages have their own request/model/manager shape, not covered by the mapping table above.
2. Add relying-party constants to `src/xbl/mod.rs::constants`.
3. Add a new top-level module (`src/bedrock/`, `src/realms/`, `src/playfab/`) mirroring `src/java/`'s layout: `model.rs`, `request.rs`, `manager.rs`.
4. Reuse `src/msa`, `src/xbl::sign`, `src/crypto`, `src/expirable::Holder` as-is — none of them are Java-Edition-specific.

**Non-goal:** the public Rust API is not meant to mirror the public Java API 1:1 (method names, builder shape). Match wire behavior, not source-level API shape — Rust idioms (async fns instead of `Holder.getUpToDate()`, `Result<T, Error>` instead of checked exceptions) are intentional deviations, not gaps to close.
