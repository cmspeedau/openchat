use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

#[derive(Debug)]
pub enum SkillError {
    Io(io::Error),
    InvalidFrontmatter(String),
    NameMismatch { expected: String, actual: String },
}

impl std::fmt::Display for SkillError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "unable to read skill: {error}"),
            Self::InvalidFrontmatter(message) => {
                write!(formatter, "invalid skill frontmatter: {message}")
            }
            Self::NameMismatch { expected, actual } => write!(
                formatter,
                "skill name `{actual}` does not match folder `{expected}`"
            ),
        }
    }
}

impl std::error::Error for SkillError {}

/// Parse YAML frontmatter fields `name` and `description` from a skill markdown file.
pub fn parse_skill(
    markdown: &str,
    folder_name: &str,
    path: impl Into<PathBuf>,
) -> Result<Skill, SkillError> {
    let mut lines = markdown.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err(SkillError::InvalidFrontmatter(
            "expected opening ---".into(),
        ));
    }
    let mut name = None;
    let mut description = None;
    let mut closed = false;
    for line in lines {
        let line = line.trim();
        if line == "---" {
            closed = true;
            break;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err(SkillError::InvalidFrontmatter(format!(
                "expected a key/value field: {line}"
            )));
        };
        let value = unquote(value.trim());
        match key.trim() {
            "name" => name = Some(value),
            "description" => description = Some(value),
            _ => {}
        }
    }
    if !closed {
        return Err(SkillError::InvalidFrontmatter("missing closing ---".into()));
    }
    let name = name
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SkillError::InvalidFrontmatter("missing name".into()))?;
    let description = description
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SkillError::InvalidFrontmatter("missing description".into()))?;
    if name != folder_name {
        return Err(SkillError::NameMismatch {
            expected: folder_name.to_owned(),
            actual: name,
        });
    }
    Ok(Skill {
        name,
        description,
        path: path.into(),
    })
}

fn unquote(value: &str) -> String {
    if value.len() >= 2 {
        let first = value.as_bytes()[0];
        let last = value.as_bytes()[value.len() - 1];
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            return value[1..value.len() - 1].to_owned();
        }
    }
    value.to_owned()
}

/// Scan supported skill roots for immediate subdirectories containing `SKILL.md`.
pub fn scan_skills(roots: &[PathBuf]) -> io::Result<Vec<Skill>> {
    let mut skills = Vec::new();
    for root in roots {
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let directory = entry.path();
            let file = directory.join("SKILL.md");
            if !file.is_file() {
                continue;
            }
            let folder_name = entry.file_name().to_string_lossy().into_owned();
            let contents = fs::read_to_string(&file)?;
            if let Ok(skill) = parse_skill(&contents, &folder_name, &file) {
                skills.push(skill);
            }
        }
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    skills.dedup_by(|left, right| left.name == right.name);
    Ok(skills)
}

/// Return the project-local and home-directory OpenCode/Claude skill roots.
pub fn default_skill_roots(project: impl AsRef<Path>) -> Vec<PathBuf> {
    let project = project.as_ref();
    let mut roots = vec![
        project.join(".opencode/skills"),
        project.join(".claude/skills"),
    ];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join(".config/opencode/skills"));
        roots.push(home.join(".claude/skills"));
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_checks_folder_name() {
        let parsed = parse_skill(
            "---\nname: demo\ndescription: 'A demo skill'\n---\nbody",
            "demo",
            "demo/SKILL.md",
        )
        .unwrap();
        assert_eq!(parsed.name, "demo");
        assert_eq!(parsed.description, "A demo skill");
        assert!(matches!(
            parse_skill("---\nname: other\ndescription: x\n---", "demo", ""),
            Err(SkillError::NameMismatch { .. })
        ));
    }

    #[test]
    fn rejects_missing_frontmatter_fields() {
        assert!(parse_skill("# no metadata", "demo", "").is_err());
        assert!(parse_skill("---\nname: demo\n---", "demo", "").is_err());
    }

    #[test]
    fn scans_skill_directories_and_skips_invalid_entries() {
        let root = tempfile::tempdir().unwrap();
        let valid = root.path().join("sample");
        fs::create_dir(&valid).unwrap();
        fs::write(
            valid.join("SKILL.md"),
            "---\nname: sample\ndescription: Sample skill\n---\n",
        )
        .unwrap();
        let invalid = root.path().join("wrong-folder");
        fs::create_dir(&invalid).unwrap();
        fs::write(
            invalid.join("SKILL.md"),
            "---\nname: not-wrong-folder\ndescription: ignored\n---",
        )
        .unwrap();
        let found = scan_skills(&[root.path().to_path_buf()]).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "sample");
    }
}
