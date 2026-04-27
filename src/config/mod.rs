pub mod schema;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::schema::ProjectConfig;

pub fn config_dir() -> PathBuf {
    // Respect XDG_CONFIG_HOME if set, otherwise always use ~/.config.
    // Avoids ~/Library/Application Support on macOS — surprising for a CLI tool.
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("portpilot")
}

pub fn projects_dir() -> PathBuf {
    config_dir().join("projects")
}

pub fn load_project(path: &Path) -> Result<ProjectConfig> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_project(&content).with_context(|| format!("parsing {}", path.display()))
}

pub fn parse_project(content: &str) -> Result<ProjectConfig> {
    let mut project: ProjectConfig = toml::from_str(content)?;
    project.normalize_and_validate()?;
    Ok(project)
}

pub fn save_project(path: &Path, config: &ProjectConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(config)?)?;
    Ok(())
}

pub fn list_projects() -> Result<Vec<PathBuf>> {
    let dir = projects_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .collect();
    paths.sort();
    Ok(paths)
}

/// Returns the path for a new project file given a stem name.
pub fn project_path(name: &str) -> PathBuf {
    projects_dir().join(format!("{name}.toml"))
}

#[cfg(test)]
mod tests {
    use super::parse_project;
    use crate::config::schema::kind;

    #[test]
    fn readme_ssh_config_loads_without_kind() {
        let project = parse_project(
            r#"
[[tunnels]]
name          = "postgres-prod"
local_port    = 5432
remote_host   = "db.internal"
remote_port   = 5432
ssh_host      = "bastion.example.com"
ssh_user      = "alice"
auto_restart  = true
"#,
        )
        .unwrap();

        assert_eq!(project.tunnels[0].kind, kind::SSH);
    }

    #[test]
    fn unknown_toml_keys_are_rejected() {
        let err = parse_project(
            r#"
[[tunnels]]
name          = "mysql-gscraper"
kind          = "kubernetes-via-bastion-ssh"
local_port    = 3306
remote_port   = 3306
bastion_host  = "15.207.252.75"
target_host   = "172.31.184.206"
taget_user    = "ec2-user"
target        = "svc/scraper-mysql-mysql"
"#,
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("unknown field `taget_user`"),
            "{err}"
        );
    }

    #[test]
    fn kubernetes_via_ssh_rejects_bastion_fields() {
        let err = parse_project(
            r#"
[[tunnels]]
name         = "mysql-gscraper"
kind         = "kubernetes-via-ssh"
local_port   = 3306
remote_port  = 3306
ssh_host     = "15.207.252.75"
ssh_user     = "ec2-user"
bastion_user = "ec2-user"
target       = "svc/scraper-mysql-mysql"
"#,
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("bastion_user is not valid for kind=kubernetes-via-ssh"),
            "{err}"
        );
    }

    #[test]
    fn all_tunnel_kinds_validate() {
        let project = parse_project(
            r#"
[[tunnels]]
name          = "ssh"
kind          = "ssh"
local_port    = 5432
remote_host   = "db.internal"
remote_port   = 5432
ssh_host      = "bastion.example.com"

[[tunnels]]
name          = "k8s"
kind          = "kubernetes"
local_port    = 8080
remote_port   = 8080
target        = "svc/api"

[[tunnels]]
name          = "k8s-ssh"
kind          = "kubernetes-via-ssh"
local_port    = 3306
remote_port   = 3306
ssh_host      = "k8s-admin.example.com"
ssh_user      = "ec2-user"
identity_file = "~/.ssh/k8s-admin.pem"
target        = "svc/mysql"
remote_user   = "deploy"

[[tunnels]]
name                  = "k8s-bastion"
kind                  = "kubernetes-via-bastion-ssh"
local_port            = 3307
remote_port           = 3306
bastion_host          = "bastion.example.com"
bastion_user          = "ec2-user"
bastion_identity_file = "~/.ssh/bastion.pem"
target_host           = "10.0.10.25"
target_user           = "ubuntu"
target_identity_file  = "~/.ssh/target.pem"
target                = "svc/mysql"
target_remote_user    = "deploy"
"#,
        )
        .unwrap();

        assert_eq!(project.tunnels.len(), 4);
    }

    #[test]
    fn required_fields_and_irrelevant_fields_are_rejected() {
        let missing_ssh_host = parse_project(
            r#"
[[tunnels]]
name        = "ssh"
kind        = "ssh"
local_port  = 5432
remote_host = "db.internal"
remote_port = 5432
"#,
        )
        .unwrap_err();
        assert!(
            missing_ssh_host
                .to_string()
                .contains("ssh_host is required"),
            "{missing_ssh_host}"
        );

        let missing_k8s_target = parse_project(
            r#"
[[tunnels]]
name        = "k8s"
kind        = "kubernetes"
local_port  = 8080
remote_port = 8080
"#,
        )
        .unwrap_err();
        assert!(
            missing_k8s_target
                .to_string()
                .contains("target is required"),
            "{missing_k8s_target}"
        );

        let missing_via_ssh_host = parse_project(
            r#"
[[tunnels]]
name        = "k8s-ssh"
kind        = "kubernetes-via-ssh"
local_port  = 3306
remote_port = 3306
target      = "svc/mysql"
"#,
        )
        .unwrap_err();
        assert!(
            missing_via_ssh_host
                .to_string()
                .contains("ssh_host is required"),
            "{missing_via_ssh_host}"
        );

        let missing_via_ssh_user = parse_project(
            r#"
[[tunnels]]
name        = "k8s-ssh"
kind        = "kubernetes-via-ssh"
local_port  = 3306
remote_port = 3306
ssh_host    = "k8s-admin.example.com"
target      = "svc/mysql"
"#,
        )
        .unwrap_err();
        assert!(
            missing_via_ssh_user
                .to_string()
                .contains("ssh_user is required"),
            "{missing_via_ssh_user}"
        );

        let missing_bastion_user = parse_project(
            r#"
[[tunnels]]
name         = "k8s-bastion"
kind         = "kubernetes-via-bastion-ssh"
local_port   = 3306
remote_port  = 3306
bastion_host = "bastion.example.com"
target_host  = "10.0.10.25"
target_user  = "ec2-user"
target       = "svc/mysql"
"#,
        )
        .unwrap_err();
        assert!(
            missing_bastion_user
                .to_string()
                .contains("bastion_user is required"),
            "{missing_bastion_user}"
        );

        let missing_bastion_target_host = parse_project(
            r#"
[[tunnels]]
name         = "k8s-bastion"
kind         = "kubernetes-via-bastion-ssh"
local_port   = 3306
remote_port  = 3306
bastion_host = "bastion.example.com"
bastion_user = "ec2-user"
target       = "svc/mysql"
"#,
        )
        .unwrap_err();
        assert!(
            missing_bastion_target_host
                .to_string()
                .contains("target_host is required"),
            "{missing_bastion_target_host}"
        );

        let missing_bastion_target_user = parse_project(
            r#"
[[tunnels]]
name         = "k8s-bastion"
kind         = "kubernetes-via-bastion-ssh"
local_port   = 3306
remote_port  = 3306
bastion_host = "bastion.example.com"
bastion_user = "ec2-user"
target_host  = "10.0.10.25"
target       = "svc/mysql"
"#,
        )
        .unwrap_err();
        assert!(
            missing_bastion_target_user
                .to_string()
                .contains("target_user is required"),
            "{missing_bastion_target_user}"
        );

        let ssh_with_target = parse_project(
            r#"
[[tunnels]]
name        = "ssh"
kind        = "ssh"
local_port  = 5432
remote_host = "db.internal"
remote_port = 5432
ssh_host    = "bastion.example.com"
target      = "svc/api"
"#,
        )
        .unwrap_err();
        assert!(
            ssh_with_target
                .to_string()
                .contains("target is not valid for kind=ssh"),
            "{ssh_with_target}"
        );

        let k8s_with_ssh = parse_project(
            r#"
[[tunnels]]
name        = "k8s"
kind        = "kubernetes"
local_port  = 8080
remote_port = 8080
target      = "svc/api"
ssh_user    = "alice"
"#,
        )
        .unwrap_err();
        assert!(
            k8s_with_ssh
                .to_string()
                .contains("ssh_user is not valid for kind=kubernetes"),
            "{k8s_with_ssh}"
        );

        let k8s_ssh_with_target_host = parse_project(
            r#"
[[tunnels]]
name        = "k8s-ssh"
kind        = "kubernetes-via-ssh"
local_port  = 3306
remote_port = 3306
ssh_host    = "k8s-admin.example.com"
ssh_user    = "ec2-user"
target_host = "10.0.10.25"
target      = "svc/mysql"
"#,
        )
        .unwrap_err();
        assert!(
            k8s_ssh_with_target_host
                .to_string()
                .contains("target_host is not valid for kind=kubernetes-via-ssh"),
            "{k8s_ssh_with_target_host}"
        );

        let bastion_with_ssh_user = parse_project(
            r#"
[[tunnels]]
name         = "k8s-bastion"
kind         = "kubernetes-via-bastion-ssh"
local_port   = 3306
remote_port  = 3306
bastion_host = "bastion.example.com"
bastion_user = "ec2-user"
target_host  = "10.0.10.25"
target_user  = "ec2-user"
target       = "svc/mysql"
ssh_user     = "alice"
"#,
        )
        .unwrap_err();
        assert!(
            bastion_with_ssh_user
                .to_string()
                .contains("ssh_user is not valid for kind=kubernetes-via-bastion-ssh"),
            "{bastion_with_ssh_user}"
        );

        let zero_port = parse_project(
            r#"
[[tunnels]]
name        = "ssh"
local_port  = 0
remote_host = "db.internal"
remote_port = 5432
ssh_host    = "bastion.example.com"
"#,
        )
        .unwrap_err();
        assert!(
            zero_port
                .to_string()
                .contains("local_port must be a number 1-65535"),
            "{zero_port}"
        );
    }
}
