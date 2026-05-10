use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use quick_xml::events::Event;
use quick_xml::reader::Reader;

/// Parsed manifest.xml representation.
#[derive(Debug)]
pub struct Manifest {
    pub remotes: Vec<Remote>,
    pub default: Option<Default>,
    pub projects: Vec<Project>,
}

/// A `<remote name="..." fetch="..." />` element.
#[derive(Debug, PartialEq)]
pub struct Remote {
    pub name: String,
    pub fetch: String,
}

/// A `<default revision="..." remote="..." />` element.
#[derive(Debug, PartialEq)]
pub struct Default {
    pub revision: Option<String>,
    pub remote: Option<String>,
}

/// A `<project path="..." name="..." />` element.
#[derive(Debug, PartialEq)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    pub revision: Option<String>,
    pub remote: Option<String>,
    pub groups: Vec<String>,
    pub size_hint: Option<SizeHint>,
    pub copyfiles: Vec<CopyFile>,
    pub linkfiles: Vec<LinkFile>,
}

/// Hint for how large a project is, used for sync priority scheduling.
#[derive(Debug, Clone, PartialEq)]
pub enum SizeHint {
    Large,
    Medium,
    Small,
}

/// A `<copyfile src="..." dest="..." />` child of `<project>`.
#[derive(Debug, Clone, PartialEq)]
pub struct CopyFile {
    pub src: String,
    pub dest: String,
}

/// A `<linkfile src="..." dest="..." />` child of `<project>`.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkFile {
    pub src: String,
    pub dest: String,
}

fn attr_value(e: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    e.attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.as_ref() == key)
        .map(|a| String::from_utf8_lossy(&a.value).into_owned())
}

fn require_attr(
    e: &quick_xml::events::BytesStart<'_>,
    key: &[u8],
    element: &str,
) -> Result<String> {
    attr_value(e, key).with_context(|| {
        format!(
            "<{element}> missing required attribute \"{}\"",
            std::str::from_utf8(key).unwrap_or("?")
        )
    })
}

/// Parse a manifest.xml string into a [`Manifest`].
pub fn parse(content: &str) -> Result<Manifest> {
    let mut reader = Reader::from_str(content);

    let mut remotes = Vec::new();
    let mut default = None;
    let mut projects = Vec::new();

    // State for tracking current project (when inside <project>...</project>)
    let mut current_project: Option<Project> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"project" => {
                    let proj = parse_project_attrs(e)?;
                    current_project = Some(proj);
                }
                b"copyfile" if current_project.is_some() => {
                    let src = require_attr(e, b"src", "copyfile")?;
                    let dest = require_attr(e, b"dest", "copyfile")?;
                    current_project.as_mut().unwrap().copyfiles.push(CopyFile { src, dest });
                }
                b"linkfile" if current_project.is_some() => {
                    let src = require_attr(e, b"src", "linkfile")?;
                    let dest = require_attr(e, b"dest", "linkfile")?;
                    current_project.as_mut().unwrap().linkfiles.push(LinkFile { src, dest });
                }
                b"remote" => {
                    let name = require_attr(e, b"name", "remote")?;
                    let fetch = require_attr(e, b"fetch", "remote")?;
                    remotes.push(Remote { name, fetch });
                }
                b"default" => {
                    default = Some(Default {
                        revision: attr_value(e, b"revision"),
                        remote: attr_value(e, b"remote"),
                    });
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) => match e.name().as_ref() {
                b"project" => {
                    projects.push(parse_project_attrs(e)?);
                }
                b"copyfile" if current_project.is_some() => {
                    let src = require_attr(e, b"src", "copyfile")?;
                    let dest = require_attr(e, b"dest", "copyfile")?;
                    current_project.as_mut().unwrap().copyfiles.push(CopyFile { src, dest });
                }
                b"linkfile" if current_project.is_some() => {
                    let src = require_attr(e, b"src", "linkfile")?;
                    let dest = require_attr(e, b"dest", "linkfile")?;
                    current_project.as_mut().unwrap().linkfiles.push(LinkFile { src, dest });
                }
                b"remote" => {
                    let name = require_attr(e, b"name", "remote")?;
                    let fetch = require_attr(e, b"fetch", "remote")?;
                    remotes.push(Remote { name, fetch });
                }
                b"default" => {
                    default = Some(Default {
                        revision: attr_value(e, b"revision"),
                        remote: attr_value(e, b"remote"),
                    });
                }
                _ => {}
            },
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == b"project"
                    && let Some(proj) = current_project.take()
                {
                    projects.push(proj);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => bail!("invalid XML at position {}: {e}", reader.error_position()),
            _ => {}
        }
    }

    Ok(Manifest {
        remotes,
        default,
        projects,
    })
}

/// Extract project attributes from a `<project ...>` element.
fn parse_project_attrs(e: &quick_xml::events::BytesStart<'_>) -> Result<Project> {
    let name = require_attr(e, b"name", "project")?;
    let path_str = attr_value(e, b"path").unwrap_or_else(|| name.clone());
    let groups = attr_value(e, b"groups")
        .map(|g| {
            g.split([',', ' '])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let size_hint = attr_value(e, b"size-hint").and_then(|s| match s.as_str() {
        "large" => Some(SizeHint::Large),
        "medium" => Some(SizeHint::Medium),
        "small" => Some(SizeHint::Small),
        _ => None,
    });
    Ok(Project {
        name,
        path: PathBuf::from(path_str),
        revision: attr_value(e, b"revision"),
        remote: attr_value(e, b"remote"),
        groups,
        size_hint,
        copyfiles: Vec::new(),
        linkfiles: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPICAL_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <remote name="origin" fetch="https://github.com/example" />
  <remote name="backup" fetch="https://backup.example.com" />
  <default revision="main" remote="origin" />
  <project path="app/core" name="core" />
  <project path="app/ui" name="ui" revision="dev" remote="backup" groups="vendor,optional" />
  <project path="lib/util" name="util" />
</manifest>
"#;

    #[test]
    fn parse_manifest_with_typical_xml_returns_all_elements() {
        let m = parse(TYPICAL_MANIFEST).unwrap();

        assert_eq!(m.remotes.len(), 2);
        assert_eq!(
            m.remotes[0],
            Remote {
                name: "origin".into(),
                fetch: "https://github.com/example".into(),
            }
        );
        assert_eq!(
            m.remotes[1],
            Remote {
                name: "backup".into(),
                fetch: "https://backup.example.com".into(),
            }
        );

        let default = m.default.as_ref().unwrap();
        assert_eq!(default.revision.as_deref(), Some("main"));
        assert_eq!(default.remote.as_deref(), Some("origin"));

        assert_eq!(m.projects.len(), 3);
        assert_eq!(m.projects[0].name, "core");
        assert_eq!(m.projects[0].path, PathBuf::from("app/core"));
        assert_eq!(m.projects[0].revision, None);
        assert!(m.projects[0].groups.is_empty());

        assert_eq!(m.projects[1].name, "ui");
        assert_eq!(m.projects[1].path, PathBuf::from("app/ui"));
        assert_eq!(m.projects[1].revision.as_deref(), Some("dev"));
        assert_eq!(m.projects[1].remote.as_deref(), Some("backup"));
        assert_eq!(m.projects[1].groups, vec!["vendor", "optional"]);
    }

    #[test]
    fn parse_manifest_with_no_default_returns_none() {
        let xml = r#"<manifest>
  <remote name="origin" fetch="https://example.com" />
  <project path="a" name="a" />
</manifest>"#;
        let m = parse(xml).unwrap();
        assert!(m.default.is_none());
    }

    #[test]
    fn parse_manifest_with_project_without_path_uses_name() {
        let xml = r#"<manifest>
  <remote name="origin" fetch="https://example.com" />
  <project name="my-lib" />
</manifest>"#;
        let m = parse(xml).unwrap();
        assert_eq!(m.projects[0].path, PathBuf::from("my-lib"));
    }

    #[test]
    fn parse_manifest_with_invalid_xml_returns_error() {
        let xml = "<manifest><remote name=";
        let result = parse(xml);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("invalid XML"),
            "expected 'invalid XML' in error, got: {msg}"
        );
    }

    #[test]
    fn parse_manifest_with_project_missing_name_returns_error() {
        let xml = r#"<manifest>
  <project path="app/core" />
</manifest>"#;
        let result = parse(xml);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("missing required attribute"),
            "expected 'missing required attribute' in error, got: {msg}"
        );
    }

    #[test]
    fn parse_manifest_with_remote_missing_fetch_returns_error() {
        let xml = r#"<manifest>
  <remote name="origin" />
</manifest>"#;
        let result = parse(xml);
        assert!(result.is_err());
    }

    #[test]
    fn parse_manifest_with_linkfile_and_copyfile() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest>
  <remote name="origin" fetch="https://example.com" />
  <default revision="main" remote="origin" />
  <project path="build" name="build-scripts">
    <linkfile src="Makefile" dest="Makefile" />
    <linkfile src="scripts/build.sh" dest="build.sh" />
    <copyfile src="config.json" dest="config.json" />
  </project>
  <project path="lib/core" name="core" />
</manifest>"#;
        let m = parse(xml).unwrap();
        assert_eq!(m.projects.len(), 2);

        let build = &m.projects[0];
        assert_eq!(build.name, "build-scripts");
        assert_eq!(build.linkfiles.len(), 2);
        assert_eq!(build.linkfiles[0], LinkFile { src: "Makefile".into(), dest: "Makefile".into() });
        assert_eq!(build.linkfiles[1], LinkFile { src: "scripts/build.sh".into(), dest: "build.sh".into() });
        assert_eq!(build.copyfiles.len(), 1);
        assert_eq!(build.copyfiles[0], CopyFile { src: "config.json".into(), dest: "config.json".into() });

        let core = &m.projects[1];
        assert!(core.linkfiles.is_empty());
        assert!(core.copyfiles.is_empty());
    }

    #[test]
    fn parse_manifest_with_self_closing_project_no_children() {
        let xml = r#"<manifest>
  <remote name="origin" fetch="https://example.com" />
  <project path="app" name="app" />
</manifest>"#;
        let m = parse(xml).unwrap();
        assert_eq!(m.projects.len(), 1);
        assert!(m.projects[0].linkfiles.is_empty());
        assert!(m.projects[0].copyfiles.is_empty());
    }
}
