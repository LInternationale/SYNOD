//! GitHub CLI integration for the create-team setup flow.

use std::{
    io,
    process::{Command, ExitStatus},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Visibility {
    Public,
    Private,
}

#[derive(Debug, Eq, PartialEq)]
pub enum InvalidRepository {
    Owner,
    Name,
}

/// Parameters shown to the user before provisioning a repository.
#[derive(Debug)]
pub struct RepositoryRequest {
    owner: String,
    name: String,
    visibility: Visibility,
}

impl RepositoryRequest {
    pub fn new(owner: &str, name: &str, visibility: Visibility) -> Result<Self, InvalidRepository> {
        if owner.is_empty()
            || owner.starts_with('-')
            || !owner
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(InvalidRepository::Owner);
        }
        if name.is_empty()
            || matches!(name, "." | "..")
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
        {
            return Err(InvalidRepository::Name);
        }
        Ok(Self {
            owner: owner.into(),
            name: name.into(),
            visibility,
        })
    }

    pub fn target(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
    pub fn visibility(&self) -> Visibility {
        self.visibility
    }

    pub fn confirm(self) -> ConfirmedRepository {
        ConfirmedRepository(self)
    }
}

/// A request explicitly confirmed in setup.
pub struct ConfirmedRepository(RepositoryRequest);

#[derive(Debug)]
pub enum CreateError {
    Launch(io::Error),
    Rejected {
        status: ExitStatus,
        diagnostic: String,
    },
}

impl ConfirmedRepository {
    fn command(&self) -> Command {
        let mut command = Command::new("gh");
        let visibility = match self.0.visibility {
            Visibility::Public => "--public",
            Visibility::Private => "--private",
        };
        command.args(["repo", "create", &self.0.target(), visibility]);
        command.env("GH_PROMPT_DISABLED", "1");
        command
    }

    pub fn create(self) -> Result<(), CreateError> {
        let output = self.command().output().map_err(CreateError::Launch)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(CreateError::Rejected {
                status: output.status,
                diagnostic: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constructs_confirmed_argument_vectors() -> Result<(), InvalidRepository> {
        for (visibility, flag) in [
            (Visibility::Public, "--public"),
            (Visibility::Private, "--private"),
        ] {
            let cmd = RepositoryRequest::new("team", "a.b-c_1", visibility)?
                .confirm()
                .command();
            assert_eq!(
                cmd.get_args().collect::<Vec<_>>(),
                ["repo", "create", "team/a.b-c_1", flag]
            );
        }
        for owner in ["", "-flag", "a/b"] {
            assert!(matches!(
                RepositoryRequest::new(owner, "a", Visibility::Private),
                Err(InvalidRepository::Owner)
            ));
        }
        for name in ["", "..", "a/b"] {
            assert!(matches!(
                RepositoryRequest::new("user", name, Visibility::Private),
                Err(InvalidRepository::Name)
            ));
        }
        Ok(())
    }
}
