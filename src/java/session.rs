use uuid::Uuid;

/// Everything needed to launch the Minecraft Java client: the fields the
/// game process expects as `--username`, `--uuid` and `--accessToken`.
#[derive(Debug, Clone)]
pub struct JavaLaunchSession {
    pub player_name: String,
    pub player_uuid: Uuid,
    pub access_token: String,
}
