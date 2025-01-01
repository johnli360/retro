use std::fmt::Display;

use axum::{
    extract::{FromRequestParts, Path},
    http::{header, request::Parts, HeaderMap, StatusCode},
    response::{self, Redirect},
};
use tracing::{error, info};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct User {
    pub name: String,
}
impl User {
    pub fn new(name: String) -> User {
        User { name }
    }
}

impl Display for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> core::fmt::Result {
        String::fmt(&self.name, f)
    }
}

impl FromRequestParts<crate::AppStateInner> for User {
    //type Rejection = Redirect;
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &crate::AppStateInner,
    ) -> Result<Self, Self::Rejection> {
        let headers = parts.headers.clone();
        let cookie = headers.get(header::COOKIE);
        let Some(name) = cookie.and_then(parse_user) else {
            info!("failed user parse: {cookie:?}");
            return Err(StatusCode::UNAUTHORIZED);
            //return Err(Redirect::permanent("/login"));
        };

        Ok(User {
            name: name.to_owned(),
        })
        //todo!()
    }
}

fn parse_user(cookie: &header::HeaderValue) -> Option<&str> {
    const SESSION_COOKIE: &str = "SESSION=";
    let cookie = cookie.to_str().ok()?;
    let (_, user) = cookie.split_once(SESSION_COOKIE)?;
    if user.contains(" ") || user.contains(";") {
        error!("TODO: improved cookie parsing");
        return None;
    }
    Some(user)
}
