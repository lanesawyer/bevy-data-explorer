//! The layering in `AGENTS.md`, checked: each layer imports only from those
//! above it, and `app`'s submodules only from `source`.
//!
//! Test code is left out, since a test may reach wherever its fixtures are,
//! and so is anything that is not a path, such as a doc link. Paths inside
//! macros such as `bsn!` are still checked, because their tokens are.

use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;
use syn::visit_mut::VisitMut;

const LAYERS: [&str; 8] = [
    "source", "render", "formats", "catalog", "widgets", "view", "bookmark", "ui",
];

/// What a file in `layer` may name after `crate::`, or `None` if anything.
fn allowed(layer: &str, file: &Path) -> Option<Vec<&'static str>> {
    if layer == "app" {
        return (file.file_name()? != "mod.rs").then(|| vec!["app", "source"]);
    }
    let rank = LAYERS.iter().position(|l| *l == layer)?;
    let mut allowed = LAYERS[..=rank].to_vec();
    if layer != "source" {
        allowed.push("app");
    }
    Some(allowed)
}

#[test]
fn no_layer_imports_from_one_below_it() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files: Vec<(PathBuf, syn::File)> = rust_files(&src)
        .into_iter()
        .map(|file| {
            let parsed = syn::parse_file(&std::fs::read_to_string(&file).unwrap()).unwrap();
            (file, parsed)
        })
        .collect();
    let test_dirs: Vec<PathBuf> = files
        .iter()
        .flat_map(|(file, parsed)| test_module_files(file, parsed))
        .collect();
    let mut violations = Vec::new();

    for (file, mut parsed) in files {
        if test_dirs.iter().any(|dir| file.starts_with(dir)) {
            continue;
        }
        let relative = file.strip_prefix(&src).unwrap();
        let layer = relative.components().next().unwrap().as_os_str();
        let Some(allowed) = allowed(layer.to_str().unwrap(), &file) else {
            continue;
        };

        StripTests.visit_file_mut(&mut parsed);

        for (target, line) in crate_paths(parsed.into_token_stream()) {
            if !allowed.contains(&target.as_str()) {
                violations.push(format!(
                    "{}:{line} names crate::{target}",
                    relative.display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "imports that climb the layering in AGENTS.md:\n  {}",
        violations.join("\n  ")
    );
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files
}

/// The files of modules declared `#[cfg(test)] mod name;`, as the paths
/// each could be at: `name.rs`, or the directory `name` holding `mod.rs`.
fn test_module_files(file: &Path, parsed: &syn::File) -> Vec<PathBuf> {
    let dir = match file.file_name().unwrap().to_str().unwrap() {
        "mod.rs" | "main.rs" => file.parent().unwrap().to_path_buf(),
        _ => file.with_extension(""),
    };
    parsed
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) if module.content.is_none() && is_test(&module.attrs) => {
                Some(module.ident.to_string())
            }
            _ => None,
        })
        .flat_map(|name| [dir.join(format!("{name}.rs")), dir.join(name)])
        .collect()
}

/// Every `crate::<module>` in the tokens, with the line it is on.
fn crate_paths(tokens: TokenStream) -> Vec<(String, usize)> {
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    let mut found = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => found.extend(crate_paths(group.stream())),
            TokenTree::Ident(ident) if ident == "crate" => {
                if let [
                    TokenTree::Punct(a),
                    TokenTree::Punct(b),
                    TokenTree::Ident(module),
                    ..,
                ] = &tokens[i + 1..]
                    && a.as_char() == ':'
                    && b.as_char() == ':'
                {
                    found.push((module.to_string(), ident.span().start().line));
                }
            }
            _ => {}
        }
    }
    found
}

struct StripTests;

fn is_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg") && {
            let tokens = attr.meta.to_token_stream().to_string();
            tokens.contains("test") && !tokens.contains("not")
        }
    })
}

impl VisitMut for StripTests {
    fn visit_file_mut(&mut self, file: &mut syn::File) {
        file.items.retain(|item| !is_test(item_attrs(item)));
        syn::visit_mut::visit_file_mut(self, file);
    }

    fn visit_item_mod_mut(&mut self, module: &mut syn::ItemMod) {
        if let Some((_, items)) = &mut module.content {
            items.retain(|item| !is_test(item_attrs(item)));
        }
        syn::visit_mut::visit_item_mod_mut(self, module);
    }

    fn visit_item_impl_mut(&mut self, block: &mut syn::ItemImpl) {
        block.items.retain(|item| {
            let attrs = match item {
                syn::ImplItem::Const(c) => &c.attrs,
                syn::ImplItem::Fn(f) => &f.attrs,
                syn::ImplItem::Type(t) => &t.attrs,
                syn::ImplItem::Macro(m) => &m.attrs,
                _ => return true,
            };
            !is_test(attrs)
        });
        syn::visit_mut::visit_item_impl_mut(self, block);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        block.stmts.retain(|stmt| match stmt {
            syn::Stmt::Item(item) => !is_test(item_attrs(item)),
            syn::Stmt::Local(local) => !is_test(&local.attrs),
            syn::Stmt::Macro(m) => !is_test(&m.attrs),
            syn::Stmt::Expr(..) => true,
        });
        syn::visit_mut::visit_block_mut(self, block);
    }
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(i) => &i.attrs,
        syn::Item::Enum(i) => &i.attrs,
        syn::Item::ExternCrate(i) => &i.attrs,
        syn::Item::Fn(i) => &i.attrs,
        syn::Item::ForeignMod(i) => &i.attrs,
        syn::Item::Impl(i) => &i.attrs,
        syn::Item::Macro(i) => &i.attrs,
        syn::Item::Mod(i) => &i.attrs,
        syn::Item::Static(i) => &i.attrs,
        syn::Item::Struct(i) => &i.attrs,
        syn::Item::Trait(i) => &i.attrs,
        syn::Item::TraitAlias(i) => &i.attrs,
        syn::Item::Type(i) => &i.attrs,
        syn::Item::Union(i) => &i.attrs,
        syn::Item::Use(i) => &i.attrs,
        _ => &[],
    }
}
