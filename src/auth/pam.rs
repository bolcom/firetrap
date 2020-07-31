//! [`Authenticator`] implementation that authenticates against [`PAM`].
//!
//! [`Authenticator`]: trait.Authenticator.html
//! [`PAM`]: https://en.wikipedia.org/wiki/Pluggable_authentication_module

use crate::auth::*;
use async_trait::async_trait;

/// [`Authenticator`] implementation that authenticates against [`PAM`].
///
/// [`Authenticator`]: ../trait.Authenticator.html
/// [`PAM`]: https://en.wikipedia.org/wiki/Pluggable_authentication_module
#[derive(Debug)]
pub struct PAMAuthenticator {
    service: String,
}

impl PAMAuthenticator {
    /// Initialize a new [`PAMAuthenticator`] for the given PAM service.
    pub fn new<S: Into<String>>(service: S) -> Self {
        let service = service.into();
        PAMAuthenticator { service }
    }
}

#[async_trait]
impl Authenticator<DefaultUser> for PAMAuthenticator {
    #[allow(clippy::type_complexity)]
    #[tracing_attributes::instrument]
    async fn authenticate(&self, username: &str, password: &str) -> Result<DefaultUser, AuthenticationError> {
        let service = self.service.clone();
        let username = username.to_string();
        let password = password.to_string();

        let mut auth = pam_auth::Authenticator::with_password(&service)?;

        auth.get_handler().set_credentials(&username, &password);
        auth.authenticate()?;
        Ok(DefaultUser {})
    }
}

impl std::convert::From<pam_auth::PamError> for AuthenticationError {
    fn from(_: pam_auth::PamError) -> Self {
        AuthenticationError
    }
}
