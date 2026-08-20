# minecraft-auth

Microsoft, Xbox Live and Minecraft Java Edition authentication for Rust.

## Features

- Login using device code or an interactive webview
- Automatic token refresh, requested lazily as you access each stage
- Serializing and deserializing the whole session to and from JSON
- Customizable application config (client id, scope, ...)

## Install

```sh
cargo add minecraft-auth
```

## Usage

### Getting started

Bring your own `reqwest::Client`:

```rust
let http_client = reqwest::Client::new();
```

### Configure the auth manager

```rust
use minecraft_auth::java::JavaAuthManager;

let builder = JavaAuthManager::builder(http_client);
```

### Logging in

#### Device code

```rust
let auth_manager = builder
    .login_device_code(|code| {
        println!("go to {}", code.verification_uri);
        println!("enter code {}", code.user_code);
    })
    .await?;

let profile = auth_manager.profile().await?;
println!("username: {}", profile.name);
```

#### Webview

`authorize` receives the Microsoft authorize URL and must resolve with the
final redirect URL once Microsoft sends the user back:

```rust
let auth_manager = builder
    .login_webview(|url| async move {
        // navigate an embedded browser to `url`, then return the URL it
        // redirects back to
        drive_webview(url).await
    })
    .await?;
```

### Saving and loading tokens

```rust
let saved = auth_manager.to_json().await?;

let auth_manager = JavaAuthManager::from_json(http_client, &saved)?;
```

### Launching the game

```rust
let session = auth_manager.launch_session().await?;
// session.player_name, session.player_uuid, session.access_token
```

## Upstream

| | |
|---|---|
| Checked against | [RaphiMC/MinecraftAuth](https://github.com/RaphiMC/MinecraftAuth) @ [`654174d`](https://github.com/RaphiMC/MinecraftAuth/commit/654174dd1f5bc27c93cf4f3bb2961664845d539c) (`5.0.3-SNAPSHOT`) |
| Ported | Java Edition login (MSA → XBL device token → XBL SISU authorize → Minecraft Services), device-code and webview-adapter MSA transports |
| Not ported | Bedrock Edition, PlayFab, Realms, Xbox Live profile/gamertag, legacy (non-title, 3-leg) XBL auth, MinecraftAuth 4.x→5.x migration |

## Commits

`type(scope): subject`, lowercase, no trailing period.

`feat` `fix` `refactor` `chore` `docs`

## License

LGPL-3.0-or-later. See [LICENSE](LICENSE).
