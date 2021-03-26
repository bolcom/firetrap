#![deny(missing_docs)]

//! Contains the [`Authenticator`](crate::auth::Authenticator) and [`UserDetail`](crate::auth::UserDetail) traits that are used by various implementations
//! and also the `Server` to authenticate users.
//!
//! Defines the common interface that can be implemented for a multitude of authentication
//! backends, e.g. *LDAP* or *PAM*. It is used by [`Server`] to authenticate users.
//!
//! You can define your own implementation to integrate your FTP(S) server with whatever
//! authentication mechanism you need. For example, to define an `Authenticator` that will randomly
//! decide:
//!
//! 1. Declare a dependency on the async-trait crate
//!
//! ```toml
//! async-trait = "0.1.42"
//! ```
//!
//! 2. Implement the [`Authenticator`] trait and optionally the [`UserDetail`] trait:
//!
//! ```no_run
//! use libunftp::auth::{Authenticator, AuthenticationError, UserDetail};
//! use async_trait::async_trait;
//! use unftp_sbe_fs::Filesystem;
//!
//! #[derive(Debug)]
//! struct RandomAuthenticator;
//!
//! #[async_trait]
//! impl Authenticator<RandomUser> for RandomAuthenticator {
//!     async fn authenticate(&self, _username: &str, _password: &str) -> Result<RandomUser, AuthenticationError> {
//!         Ok(RandomUser{})
//!     }
//! }
//!
//! #[derive(Debug)]
//! struct RandomUser;
//!
//! impl UserDetail for RandomUser {}
//!
//! impl std::fmt::Display for RandomUser {
//!     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//!         write!(f, "RandomUser")
//!     }
//! }
//! ```
//!
//! 3. Initialize it with the server:
//!
//! ```
//! # // Make it compile
//! # type RandomAuthenticator = libunftp::auth::AnonymousAuthenticator;
//! let server = libunftp::Server::with_authenticator(
//!   Box::new(move || { unftp_sbe_fs::Filesystem::new("/srv/ftp") }),
//!   std::sync::Arc::new(RandomAuthenticator{})
//! );
//! ```
//!
//! [`Server`]: ../struct.Server.html
//! [`Authenticator`]: trait.Authenticator.html
//! [`UserDetail`]: trait.UserDetail.html
//!
pub mod anonymous;
pub use anonymous::AnonymousAuthenticator;

pub(crate) mod authenticator;
#[allow(unused_imports)]
pub use authenticator::{AuthenticationError, Authenticator};

mod user;
pub use user::{DefaultUser, UserDetail};
