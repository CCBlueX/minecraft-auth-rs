use std::time::SystemTime;
use tokio::sync::OnceCell;

static OFFSET_SECONDS: OnceCell<i64> = OnceCell::const_new();

/// Seconds to add to the local clock to match Microsoft's servers, sampled
/// once from `login.live.com`'s `Date` header. Xbox Live rejects signed
/// requests whose timestamp drifts too far, so a wrong local clock would
/// otherwise break every signed request.
pub async fn offset_seconds(client: &reqwest::Client) -> i64 {
    *OFFSET_SECONDS
        .get_or_init(|| async { fetch_offset(client).await.unwrap_or(0) })
        .await
}

async fn fetch_offset(client: &reqwest::Client) -> Option<i64> {
    let response = client
        .get("https://login.live.com/")
        .timeout(crate::REQUEST_TIMEOUT)
        .send()
        .await
        .ok()?;
    let date_header = response
        .headers()
        .get(reqwest::header::DATE)?
        .to_str()
        .ok()?;
    let server_time = httpdate::parse_http_date(date_header).ok()?;
    let client_time = SystemTime::now();
    Some(match server_time.duration_since(client_time) {
        Ok(ahead) => ahead.as_secs() as i64,
        Err(behind) => -(behind.duration().as_secs() as i64),
    })
}
