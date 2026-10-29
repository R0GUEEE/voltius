use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
pub enum Jail {
    NotFound,
    Outside,
}

#[derive(Debug, PartialEq)]
pub enum Resolved {
    VirtualRoot,
    Root(usize),
    Inside { root: usize, path: PathBuf },
}

struct Root {
    name: String,
    path: PathBuf,
}

pub struct SharedRoots {
    roots: Vec<Root>,
}

fn base_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn normalize(vpath: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for seg in vpath.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out
}

fn is_host_specific(seg: &str) -> bool {
    seg.contains('\0')
        || (cfg!(windows) && (seg.contains(':') || seg.contains('\\')))
        || seg.contains(std::path::MAIN_SEPARATOR)
}

impl SharedRoots {
    pub fn new(folders: &[PathBuf]) -> Self {
        let mut roots: Vec<Root> = Vec::with_capacity(folders.len());
        for path in folders {
            let base = base_name(path);
            let mut name = base.clone();
            let mut n = 2;
            while roots.iter().any(|r| r.name == name) {
                name = format!("{base} ({n})");
                n += 1;
            }
            roots.push(Root {
                name,
                path: path.clone(),
            });
        }
        Self { roots }
    }

    pub fn names(&self) -> Vec<String> {
        self.roots.iter().map(|r| r.name.clone()).collect()
    }

    fn locate<'a>(&self, comps: &'a [&'a str]) -> Result<(usize, PathBuf, &'a [&'a str]), Jail> {
        let (first, rest) = comps.split_first().expect("non-empty components");
        if comps.iter().any(|c| is_host_specific(c)) {
            return Err(Jail::Outside);
        }
        let idx = self
            .roots
            .iter()
            .position(|r| r.name == *first)
            .ok_or(Jail::NotFound)?;
        let canon = self.roots[idx]
            .path
            .canonicalize()
            .map_err(|_| Jail::NotFound)?;
        Ok((idx, canon, rest))
    }

    pub fn resolve(&self, vpath: &str) -> Result<Resolved, Jail> {
        let comps = normalize(vpath);
        if comps.is_empty() {
            return Ok(Resolved::VirtualRoot);
        }
        let (root, canon, rest) = self.locate(&comps)?;
        if rest.is_empty() {
            return Ok(Resolved::Root(root));
        }
        let target = canon
            .join(rest.iter().collect::<PathBuf>())
            .canonicalize()
            .map_err(|_| Jail::NotFound)?;
        if !target.starts_with(&canon) {
            return Err(Jail::Outside);
        }
        Ok(Resolved::Inside { root, path: target })
    }

    pub fn resolve_for_create(&self, vpath: &str) -> Result<Resolved, Jail> {
        let comps = normalize(vpath);
        if comps.len() < 2 {
            return Err(Jail::Outside);
        }
        let (parent_comps, leaf) = comps.split_at(comps.len() - 1);
        let leaf = leaf[0];
        let (root, canon, rest) = self.locate(parent_comps)?;
        let parent = canon
            .join(rest.iter().collect::<PathBuf>())
            .canonicalize()
            .map_err(|_| Jail::NotFound)?;
        if !parent.starts_with(&canon) {
            return Err(Jail::Outside);
        }
        let path = parent.join(leaf);
        if path.symlink_metadata().is_ok() {
            let real = path.canonicalize().map_err(|_| Jail::Outside)?;
            if !real.starts_with(&canon) {
                return Err(Jail::Outside);
            }
        }
        Ok(Resolved::Inside { root, path })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::fs;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        outside: PathBuf,
        roots: SharedRoots,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("share");
        let outside = dir.path().join("secret");
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("pw"), "x").unwrap();
        fs::write(root.join("a/f.txt"), "x").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
            std::os::unix::fs::symlink(root.join("a"), root.join("inner")).unwrap();
        }
        let roots = SharedRoots::new(std::slice::from_ref(&root));
        Fixture {
            _dir: dir,
            root: root.canonicalize().unwrap(),
            outside,
            roots,
        }
    }

    #[test]
    fn dotdot_never_leaves_root() {
        let f = fixture();
        assert_eq!(f.roots.resolve("/..").unwrap(), Resolved::VirtualRoot);
        assert_eq!(f.roots.resolve("/../../..").unwrap(), Resolved::VirtualRoot);
        assert_eq!(f.roots.resolve("/share/..").unwrap(), Resolved::VirtualRoot);
        assert_eq!(
            f.roots.resolve("/share/a/../../..").unwrap(),
            Resolved::VirtualRoot
        );
        assert_eq!(
            f.roots.resolve("/share/a/../a//./b").unwrap(),
            Resolved::Inside {
                root: 0,
                path: f.root.join("a/b")
            }
        );
        assert_eq!(f.roots.resolve("").unwrap(), Resolved::VirtualRoot);
        assert_eq!(f.roots.resolve("/share").unwrap(), Resolved::Root(0));
    }

    #[test]
    fn absolute_host_path_is_outside() {
        let f = fixture();
        let host = f.outside.to_string_lossy().into_owned();
        assert!(f.roots.resolve(&host).is_err());
        assert!(f.roots.resolve(&format!("/share{host}")).is_err());
        assert_eq!(f.roots.resolve("/nope"), Err(Jail::NotFound));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_root_is_outside() {
        let f = fixture();
        assert_eq!(f.roots.resolve("/share/link"), Err(Jail::Outside));
        assert_eq!(f.roots.resolve("/share/link/pw"), Err(Jail::Outside));
        assert_eq!(
            f.roots.resolve_for_create("/share/link/new"),
            Err(Jail::Outside)
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_through_leaf_symlink_out_of_root_is_outside() {
        let f = fixture();
        assert_eq!(
            f.roots.resolve_for_create("/share/link"),
            Err(Jail::Outside)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_inside_root_is_followed() {
        let f = fixture();
        assert_eq!(
            f.roots.resolve("/share/inner/f.txt").unwrap(),
            Resolved::Inside {
                root: 0,
                path: f.root.join("a/f.txt")
            }
        );
    }

    #[test]
    fn duplicate_names_get_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let folders: Vec<PathBuf> = ["x", "y", "z"]
            .iter()
            .map(|d| {
                let p = dir.path().join(d).join("docs");
                fs::create_dir_all(&p).unwrap();
                p
            })
            .collect();
        let roots = SharedRoots::new(&folders);
        assert_eq!(roots.names(), vec!["docs", "docs (2)", "docs (3)"]);
        assert_eq!(roots.resolve("/docs (3)").unwrap(), Resolved::Root(2));
    }

    #[test]
    fn missing_root_lists_but_errors_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone");
        let roots = SharedRoots::new(&[gone]);
        assert_eq!(roots.names(), vec!["gone"]);
        assert_eq!(roots.resolve("/gone"), Err(Jail::NotFound));
        assert_eq!(roots.resolve("/gone/x"), Err(Jail::NotFound));
        assert_eq!(roots.resolve_for_create("/gone/x"), Err(Jail::NotFound));
    }

    #[test]
    fn create_rejects_separator_in_leaf() {
        let f = fixture();
        assert_eq!(f.roots.resolve_for_create("/share"), Err(Jail::Outside));
        assert_eq!(f.roots.resolve_for_create("/share/.."), Err(Jail::Outside));
        assert_eq!(
            f.roots.resolve_for_create("/share/a/new.txt").unwrap(),
            Resolved::Inside {
                root: 0,
                path: f.root.join("a/new.txt")
            }
        );
        assert_eq!(
            f.roots.resolve_for_create("/share/missing/new.txt"),
            Err(Jail::NotFound)
        );
        assert_eq!(
            f.roots
                .resolve_for_create(&format!("/share/a/x{}y", std::path::MAIN_SEPARATOR)),
            if std::path::MAIN_SEPARATOR == '/' {
                Err(Jail::NotFound)
            } else {
                Err(Jail::Outside)
            }
        );
    }

    #[cfg(windows)]
    #[test]
    fn drive_and_unc_leaf_are_outside() {
        let f = fixture();
        for leaf in ["C:", "C:evil", "\\\\host\\share", "a\\b", "x:y"] {
            assert_eq!(
                f.roots.resolve_for_create(&format!("/share/a/{leaf}")),
                Err(Jail::Outside)
            );
            assert_eq!(
                f.roots.resolve(&format!("/share/a/{leaf}")),
                Err(Jail::Outside)
            );
        }
    }

    proptest! {
        #[test]
        fn no_generated_path_escapes(
            segs in proptest::collection::vec(
                proptest::sample::select(vec!["..", ".", "a", "b", "link", "", "%2e%2e", "\u{2025}"]),
                0..8,
            )
        ) {
            let f = fixture();
            let vpath = format!("/share/{}", segs.join("/"));
            for r in [f.roots.resolve(&vpath), f.roots.resolve_for_create(&vpath)] {
                if let Ok(Resolved::Inside { path, .. }) = r {
                    let real = path
                        .canonicalize()
                        .unwrap_or_else(|_| path.parent().unwrap().canonicalize().unwrap());
                    prop_assert!(real.starts_with(&f.root));
                }
            }
        }
    }
}
