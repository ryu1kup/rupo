use serde::{Deserialize, Serialize};

use crate::manifest::xml;

/// Native rupo manifest in TOML format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaults: Option<Defaults>,
    pub remotes: Vec<RemoteEntry>,
    pub projects: Vec<ProjectEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteEntry {
    pub name: String,
    pub fetch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub path: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copyfiles: Vec<FileCopy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linkfiles: Vec<FileLink>,
}

/// A file copy rule: copy `src` (relative to project) to `dest` (relative to workspace root).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCopy {
    pub src: String,
    pub dest: String,
}

/// A symlink rule: create symlink at `dest` pointing to `src` (relative to project).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileLink {
    pub src: String,
    pub dest: String,
}

impl Manifest {
    /// Convert from a parsed XML manifest, optionally overriding the default revision.
    pub fn from_xml(xml_manifest: &xml::Manifest, branch: Option<&str>) -> Self {
        let defaults = {
            let revision = branch.map(String::from).or_else(|| {
                xml_manifest
                    .default
                    .as_ref()
                    .and_then(|d| d.revision.clone())
            });
            let remote = xml_manifest.default.as_ref().and_then(|d| d.remote.clone());
            if revision.is_some() || remote.is_some() {
                Some(Defaults { revision, remote })
            } else {
                None
            }
        };

        let remotes = xml_manifest
            .remotes
            .iter()
            .map(|r| RemoteEntry {
                name: r.name.clone(),
                fetch: r.fetch.clone(),
            })
            .collect();

        let projects = xml_manifest
            .projects
            .iter()
            .map(|p| ProjectEntry {
                path: p.path.to_string_lossy().into_owned(),
                name: p.name.clone(),
                revision: p.revision.clone(),
                remote: p.remote.clone(),
                groups: p.groups.clone(),
                copyfiles: p.copyfiles.iter().map(|c| FileCopy {
                    src: c.src.clone(),
                    dest: c.dest.clone(),
                }).collect(),
                linkfiles: p.linkfiles.iter().map(|l| FileLink {
                    src: l.src.clone(),
                    dest: l.dest.clone(),
                }).collect(),
            })
            .collect();

        Manifest {
            defaults,
            remotes,
            projects,
        }
    }
}

/// Parse a TOML string into a Manifest.
pub fn parse(content: &str) -> anyhow::Result<Manifest> {
    let manifest: Manifest = ::toml::from_str(content)?;
    Ok(manifest)
}
