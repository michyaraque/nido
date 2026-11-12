use super::*;

pub fn pct_decode(s: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'+' => o.push(b' '),
            b'%' if i + 2 < s.len() => {
                match std::str::from_utf8(&s[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        o.push(v);
                        i += 2;
                    }
                    None => o.push(b'%'),
                }
            }
            c => o.push(c),
        }
        i += 1;
    }
    o
}

/// UTF-8, or Windows-1252/Latin-1 as fallback (old clients, consoles): "Caf\xE9" → "Café", never "Caf�".
pub fn to_text(b: Vec<u8>) -> String {
    String::from_utf8(b).unwrap_or_else(|e| {
        e.into_bytes().into_iter().map(|c| if c == 0x80 { '€' } else { c as char }).collect()
    })
}

pub struct F<T>(pub T);
impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for F<T> {
    type Rejection = Response;
    async fn from_request(req: Request, state: &S) -> Result<Self, Response> {
        let body = Bytes::from_request(req, state).await.map_err(IntoResponse::into_response)?;
        let pairs: Vec<(String, String)> = body
            .split(|&c| c == b'&')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let mut kv = p.splitn(2, |&c| c == b'=');
                let k = to_text(pct_decode(kv.next().unwrap_or_default()));
                (k, to_text(pct_decode(kv.next().unwrap_or_default())))
            })
            .collect();
        let qs = serde_urlencoded::to_string(&pairs).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()).into_response())?;
        serde_urlencoded::from_str(&qs)
            .map(F)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("Formulario no válido: {e}")).into_response())
    }
}

pub fn clean(s: &str) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(100).collect()
}
