//! Owned by: W0-A
//! Owner-tag validation and syntax-aware read-only source checks.

use anyhow::{bail, Context, Result};
use std::{fs, path::Path};
use syn::{visit::Visit, Expr, Item};

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Path>()
                .is_ok_and(|path| path.is_ident("test"))
    })
}

#[derive(Default)]
struct Forbidden {
    hits: Vec<String>,
}

impl<'ast> Visit<'ast> for Forbidden {
    fn visit_item(&mut self, item: &'ast Item) {
        let attrs = match item {
            Item::Const(i) => &i.attrs,
            Item::Enum(i) => &i.attrs,
            Item::ExternCrate(i) => &i.attrs,
            Item::Fn(i) => &i.attrs,
            Item::ForeignMod(i) => &i.attrs,
            Item::Impl(i) => &i.attrs,
            Item::Macro(i) => &i.attrs,
            Item::Mod(i) => &i.attrs,
            Item::Static(i) => &i.attrs,
            Item::Struct(i) => &i.attrs,
            Item::Trait(i) => &i.attrs,
            Item::TraitAlias(i) => &i.attrs,
            Item::Type(i) => &i.attrs,
            Item::Union(i) => &i.attrs,
            Item::Use(i) => &i.attrs,
            _ => return,
        };
        if !test_only(attrs) {
            syn::visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let attrs = match item {
            syn::ImplItem::Const(i) => &i.attrs,
            syn::ImplItem::Fn(i) => &i.attrs,
            syn::ImplItem::Type(i) => &i.attrs,
            syn::ImplItem::Macro(i) => &i.attrs,
            _ => return,
        };
        if !test_only(attrs) {
            syn::visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        let attrs = match item {
            syn::TraitItem::Const(i) => &i.attrs,
            syn::TraitItem::Fn(i) => &i.attrs,
            syn::TraitItem::Type(i) => &i.attrs,
            syn::TraitItem::Macro(i) => &i.attrs,
            _ => return,
        };
        if !test_only(attrs) {
            syn::visit::visit_trait_item(self, item);
        }
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let parts: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        if parts.iter().any(|p| {
            matches!(
                p.as_str(),
                "FS_IOC_FSSETXATTR" | "Q_SETQUOTA" | "Q_XSETQLIM"
            )
        }) || parts
            .windows(2)
            .any(|p| p[0] == "libc" && matches!(p[1].as_str(), "mount" | "umount" | "umount2"))
        {
            self.hits.push(parts.join("::"));
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(func) = call.func.as_ref() {
            let mut parts = func.path.segments.iter().rev();
            let is_new = parts.next().is_some_and(|p| p.ident == "new");
            let is_command = parts.next().is_some_and(|p| p.ident == "Command");
            if is_new && is_command {
                if let Some(Expr::Lit(lit)) = call.args.first() {
                    if let syn::Lit::Str(command) = &lit.lit {
                        let command = command.value();
                        let name = command.rsplit('/').next().unwrap_or(&command);
                        if ["lv", "vg", "pv"]
                            .iter()
                            .any(|prefix| name.starts_with(prefix))
                            && !matches!(name, "lvs" | "vgs")
                        {
                            self.hits.push(format!("Command::new({command:?})"));
                        }
                    }
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        // Rust macros contain token trees, not AST children. Parse each group as
        // an expression when possible, then inspect remaining identifier tokens.
        self.scan_tokens(mac.tokens.clone());
    }
}

impl Forbidden {
    fn scan_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        if let Ok(expr) = syn::parse2::<Expr>(tokens.clone()) {
            self.visit_expr(&expr);
            return;
        }
        let tokens: Vec<_> = tokens.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            // Attempt a path or call beginning at every identifier, including
            // macro argument lists that are not themselves a single expression.
            if matches!(token, proc_macro2::TokenTree::Ident(_)) {
                for len in [4, 5] {
                    if let Some(slice) = tokens.get(index..index + len) {
                        let stream = slice.iter().cloned().collect();
                        if let Ok(expr) = syn::parse2::<Expr>(stream) {
                            self.visit_expr(&expr);
                        }
                    }
                }
            }
            match token {
                proc_macro2::TokenTree::Group(group) => self.scan_tokens(group.stream()),
                proc_macro2::TokenTree::Ident(ident)
                    if matches!(
                        ident.to_string().as_str(),
                        "FS_IOC_FSSETXATTR" | "Q_SETQUOTA" | "Q_XSETQLIM"
                    ) =>
                {
                    self.hits.push(ident.to_string());
                }
                _ => {}
            }
        }
    }
}

fn test_module_paths(tree: &syn::File, path: &Path) -> Vec<std::path::PathBuf> {
    let parent = path.parent().unwrap_or(Path::new(""));
    let base = match path.file_stem().and_then(|s| s.to_str()) {
        Some("lib" | "main" | "mod") => parent.to_path_buf(),
        Some(stem) => parent.join(stem),
        None => parent.to_path_buf(),
    };
    let mut pending = vec![(&tree.items, base, false)];
    let mut excluded = Vec::new();
    while let Some((items, base, inherited_test)) = pending.pop() {
        for item in items {
            if let Item::Mod(module) = item {
                let is_test = inherited_test || test_only(&module.attrs);
                let dir = base.join(module.ident.to_string());
                if let Some((_, items)) = &module.content {
                    pending.push((items, dir, is_test));
                } else if is_test {
                    let explicit = module.attrs.iter().find_map(|attr| {
                        if !attr.path().is_ident("path") {
                            return None;
                        }
                        if let syn::Meta::NameValue(value) = &attr.meta {
                            if let Expr::Lit(lit) = &value.value {
                                if let syn::Lit::Str(value) = &lit.lit {
                                    return Some(base.join(value.value()));
                                }
                            }
                        }
                        None
                    });
                    if let Some(path) = explicit {
                        excluded.push(path);
                    } else {
                        excluded.push(dir.with_extension("rs"));
                        excluded.push(dir);
                    }
                }
            }
        }
    }
    excluded
}

pub(crate) fn check_sources(root: &Path) -> Result<()> {
    let mut pending = Vec::new();
    for entry in fs::read_dir(root.join("crates"))? {
        pending.push(entry?.path().join("src"));
    }
    let mut count = 0;
    let mut trees = Vec::new();
    let mut excluded = Vec::new();
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_symlink() {
            bail!("source symlink is not supported: {}", path.display());
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(&path)? {
                pending.push(entry?.path());
            }
            continue;
        }
        let source = fs::read_to_string(&path)?;
        if !source
            .lines()
            .any(|line| line.starts_with("//! Owned by: W"))
        {
            bail!("missing owner tag: {}", path.display());
        }
        count += 1;
        if path.extension().is_some_and(|ext| ext == "rs")
            && !path
                .components()
                .any(|c| c.as_os_str() == "test-support" || c.as_os_str() == "tests")
        {
            let tree =
                syn::parse_file(&source).with_context(|| format!("parse {}", path.display()))?;
            excluded.extend(test_module_paths(&tree, &path));
            trees.push((path, tree));
        }
    }
    for (path, tree) in trees {
        if excluded.iter().any(|test_path| path.starts_with(test_path)) {
            continue;
        }
        let mut scanner = Forbidden::default();
        scanner.visit_file(&tree);
        if !scanner.hits.is_empty() {
            bail!(
                "forbidden read-only symbols in {}: {}",
                path.display(),
                scanner.hits.join(", ")
            );
        }
    }
    println!("source checks: {count} owner tags; forbidden-symbol scan passed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(source: &str) -> Vec<String> {
        let tree = syn::parse_file(source).unwrap();
        let mut scanner = Forbidden::default();
        scanner.visit_file(&tree);
        scanner.hits
    }

    #[test]
    fn identifies_external_test_modules() {
        let tree = syn::parse_file(
            "#[cfg(test)] mod unit; #[cfg(test)] #[path = \"checks.rs\"] mod other; mod production;"
        ).unwrap();
        let excluded = test_module_paths(&tree, Path::new("crates/demo/src/lib.rs"));
        assert!(excluded.contains(&Path::new("crates/demo/src/unit.rs").to_path_buf()));
        assert!(excluded.contains(&Path::new("crates/demo/src/unit").to_path_buf()));
        assert!(excluded.contains(&Path::new("crates/demo/src/checks.rs").to_path_buf()));
        assert!(!excluded.contains(&Path::new("crates/demo/src/production.rs").to_path_buf()));
    }

    #[test]
    fn detects_forbidden_symbols_and_commands() {
        for source in [
            "fn f() { libc::mount(); }",
            "fn f() { libc::umount2(); }",
            "fn f() { let x = FS_IOC_FSSETXATTR; }",
            "fn f() { let x = Q_SETQUOTA; }",
            "fn f() { let x = Q_XSETQLIM; }",
            "fn f() { std::process::Command::new(\"/sbin/lvcreate\"); }",
            "fn f() { Command::new(\"vgremove\"); }",
            "fn f() { Command::new(\"pvcreate\"); }",
            "fn f() { wrapper!(libc::mount()); }",
            "fn f() { wrapper!(a, libc::mount(), Command::new(\"lvremove\")); }",
        ] {
            assert!(!hits(source).is_empty(), "{source}");
        }
    }

    #[test]
    fn excludes_tests_comments_and_literals_but_allows_reports() {
        assert!(hits(
            r###"
            // libc::mount(); FS_IOC_FSSETXATTR
            fn f() {
                let s = r#"Q_SETQUOTA libc::mount()"#;
                Command::new("lvs"); Command::new("/sbin/vgs");
                log!("FS_IOC_FSSETXATTR");
            }
            #[cfg(test)] mod tests { fn f() { libc::mount(); Command::new("lvcreate"); } }
        "###
        )
        .is_empty());
    }
}
