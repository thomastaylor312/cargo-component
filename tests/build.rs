use crate::support::*;
use anyhow::{Context, Result};
use assert_cmd::prelude::*;
use predicates::{prelude::PredicateBooleanExt, str::contains};
use std::fs;
use toml_edit::{value, Item, Table};

mod support;

#[test]
fn it_builds_debug() -> Result<()> {
    let project = Project::new("foo")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    project
        .cargo_component("build")
        .assert()
        .stderr(contains("Finished dev [unoptimized + debuginfo] target(s)"))
        .success();

    validate_component(&project.debug_wasm("foo"))?;

    // A lock file should only be generated for projects with
    // registry dependencies
    assert!(!project.root().join("Cargo-component.lock").exists());

    Ok(())
}

#[test]
fn it_builds_a_bin_project() -> Result<()> {
    let project = Project::new_bin("foo")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    project
        .cargo_component("build --release")
        .assert()
        .stderr(contains("Finished release [optimized] target(s)"))
        .success();

    validate_component(&project.release_wasm("foo"))?;

    Ok(())
}

#[test]
fn it_builds_a_workspace() -> Result<()> {
    let project = project()?
        .file(
            "Cargo.toml",
            r#"[workspace]
members = ["foo", "bar", "baz"]
"#,
        )?
        .file(
            "baz/Cargo.toml",
            r#"[package]
name = "baz"
version = "0.1.0"
edition = "2021"

[dependencies]
"#,
        )?
        .file("baz/src/lib.rs", "")?
        .build();

    project
        .cargo_component("new --reactor foo")
        .assert()
        .stderr(contains("Updated manifest of package `foo`"))
        .success();

    let member = ProjectBuilder::new(project.root().join("foo")).build();
    member.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    project
        .cargo_component("new --reactor bar")
        .assert()
        .stderr(contains("Updated manifest of package `bar`"))
        .success();

    let member = ProjectBuilder::new(project.root().join("bar")).build();
    member.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    project
        .cargo_component("build")
        .assert()
        .stderr(contains("Finished dev [unoptimized + debuginfo] target(s)"))
        .success();

    validate_component(&project.debug_wasm("foo"))?;
    validate_component(&project.debug_wasm("bar"))?;

    Ok(())
}

#[test]
fn it_supports_wit_keywords() -> Result<()> {
    let project = Project::new("interface")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    project
        .cargo_component("build --release")
        .assert()
        .stderr(contains("Finished release [optimized] target(s)"))
        .success();

    validate_component(&project.release_wasm("interface"))?;

    Ok(())
}

#[test]
fn it_adds_a_producers_field() -> Result<()> {
    let project = Project::new("foo")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    project
        .cargo_component("build --release")
        .assert()
        .stderr(contains("Finished release [optimized] target(s)"))
        .success();

    let path = project.release_wasm("foo");

    validate_component(&path)?;

    let wasm = fs::read(&path)
        .with_context(|| format!("failed to read wasm file `{path}`", path = path.display()))?;
    let section = wasm_metadata::Producers::from_wasm(&wasm)?.expect("missing producers section");

    assert_eq!(
        section
            .get("processed-by")
            .expect("missing processed-by field")
            .get(env!("CARGO_PKG_NAME"))
            .expect("missing cargo-component field"),
        option_env!("CARGO_VERSION_INFO").unwrap_or(env!("CARGO_PKG_VERSION"))
    );

    Ok(())
}

#[test]
fn it_builds_wasm32_unknown_unknown() -> Result<()> {
    let project = Project::new("foo")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    project
        .cargo_component("build --target wasm32-unknown-unknown")
        .assert()
        .stderr(contains("Finished dev [unoptimized + debuginfo] target(s)"))
        .success();

    validate_component(
        &project
            .build_dir()
            .join("wasm32-unknown-unknown")
            .join("debug")
            .join("foo.wasm"),
    )?;

    Ok(())
}

#[test]
fn it_regenerates_target_if_wit_changed() -> Result<()> {
    let project = Project::new("foo")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        doc["package"]["metadata"]["component"]["target"]["world"] = value("example");
        Ok(doc)
    })?;

    project
        .cargo_component("build")
        .assert()
        .stderr(contains("Finished dev [unoptimized + debuginfo] target(s)"))
        .success();

    validate_component(&project.debug_wasm("foo"))?;

    project
        .cargo_component("build")
        .assert()
        .stderr(contains("Encoding target").not())
        .success();

    fs::write(project.root().join("wit/other.wit"), "world foo {}")?;

    project
        .cargo_component("build")
        .assert()
        .stderr(contains("Encoding target"))
        .success();

    Ok(())
}

#[test]
fn it_builds_with_local_wit_deps() -> Result<()> {
    let project = Project::new("foo")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        let mut dependencies = Table::new();
        dependencies["foo:bar"]["path"] = value("wit/deps/foo-bar");
        dependencies["bar:baz"]["path"] = value("wit/deps/bar-baz/qux.wit");
        dependencies["baz:qux"]["path"] = value("wit/deps/foo-bar/deps/baz-qux/qux.wit");

        let target =
            doc["package"]["metadata"]["component"]["target"].or_insert(Item::Table(Table::new()));
        target["dependencies"] = Item::Table(dependencies);
        Ok(doc)
    })?;

    // Create the foo-bar wit package
    fs::create_dir_all(project.root().join("wit/deps/foo-bar/deps/baz-qux"))?;
    fs::write(
        project.root().join("wit/deps/foo-bar/deps/baz-qux/qux.wit"),
        "package baz:qux

interface qux {
    type ty = u32
}",
    )?;

    fs::write(
        project.root().join("wit/deps/foo-bar/bar.wit"),
        "package foo:bar

interface baz {
    use baz:qux/qux.{ty}
    baz: func() -> ty
}",
    )?;

    fs::create_dir_all(project.root().join("wit/deps/bar-baz"))?;

    fs::write(
        project.root().join("wit/deps/bar-baz/qux.wit"),
        "package bar:baz
interface qux {
    use baz:qux/qux.{ty}
    qux: func()
}",
    )?;

    fs::write(
        project.root().join("wit/world.wit"),
        "package component:foo

world example {
    export foo:bar/baz
    export bar:baz/qux
}",
    )?;

    fs::write(
        project.root().join("src/lib.rs"),
        "cargo_component_bindings::generate!();
use bindings::exports::{foo::bar::baz::{Guest as Baz, Ty}, bar::baz::qux::Guest as Qux};

struct Component;

impl Baz for Component {
    fn baz() -> Ty {
        todo!()
    }
}

impl Qux for Component {
    fn qux() {
        todo!()
    }
}
",
    )?;

    project
        .cargo_component("build")
        .assert()
        .stderr(contains("Finished dev [unoptimized + debuginfo] target(s)"))
        .success();

    validate_component(&project.debug_wasm("foo"))?;

    Ok(())
}

#[test]
fn it_builds_with_a_specified_implementor() -> Result<()> {
    let project = Project::new("foo")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    fs::write(
        project.root().join("src/lib.rs"),
        r#"cargo_component_bindings::generate!({
    implementor: CustomImplementor
});

use bindings::Guest;

struct CustomImplementor;

impl Guest for CustomImplementor {
    fn hello_world() -> String {
        todo!()
    }
}
"#,
    )?;

    project
        .cargo_component("build")
        .assert()
        .stderr(contains("Finished dev [unoptimized + debuginfo] target(s)"))
        .success();

    validate_component(&project.debug_wasm("foo"))?;

    Ok(())
}

#[test]
fn empty_world_with_dep_valid() -> Result<()> {
    let project = Project::new("dep")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    fs::write(
        project.root().join("wit/world.wit"),
        "
            package foo:bar

            world the-world {
                flags foo {
                    bar
                }

                export hello: func() -> foo
            }
        ",
    )?;

    fs::write(
        project.root().join("src/lib.rs"),
        "
            cargo_component_bindings::generate!();
            use bindings::{Guest, Foo};
            struct Component;

            impl Guest for Component {
                fn hello() -> Foo {
                    Foo::BAR
                }
            }
        ",
    )?;

    project.cargo_component("build").assert().success();

    let dep = project.debug_wasm("dep");
    validate_component(&dep)?;

    let project = Project::with_root(project.root().parent().unwrap(), "main", "")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        let table = doc["package"]["metadata"]["component"]
            .as_table_mut()
            .unwrap();
        table.remove("package");
        table.remove("target");
        let mut dependencies = Table::new();
        dependencies["foo:bar"]["path"] = value(dep.display().to_string());
        doc["package"]["metadata"]["component"]["dependencies"] = Item::Table(dependencies);
        Ok(doc)
    })?;

    fs::remove_dir_all(project.root().join("wit"))?;

    fs::write(
        project.root().join("src/lib.rs"),
        "
            cargo_component_bindings::generate!();

            #[no_mangle]
            pub extern \"C\" fn foo() {
                bindings::bar::hello();
            }
        ",
    )?;

    project.cargo_component("build").assert().success();
    validate_component(&project.debug_wasm("main"))?;

    Ok(())
}

#[test]
fn it_builds_with_resources() -> Result<()> {
    let project = Project::new("foo")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    fs::write(
        project.root().join("wit/world.wit"),
        "
            package foo:bar

            world bar {
                export baz: interface {
                    resource keyed-integer {
                        constructor(x: u32)
                        get: func() -> u32
                        set: func(x: u32)
                        key: static func() -> string
                    }
                }
            }
        ",
    )?;

    fs::write(
        project.root().join("src/lib.rs"),
        r#"
            cargo_component_bindings::generate!();

            use std::cell::Cell;

            pub struct KeyedInteger(Cell<u32>);

            impl bindings::exports::baz::GuestKeyedInteger for KeyedInteger {
                fn new(x: u32) -> Self {
                    Self(Cell::new(x))
                }

                fn get(&self) -> u32 {
                    self.0.get()
                }

                fn set(&self, x: u32) {
                    self.0.set(x);
                }

                fn key() -> String {
                    "my-key".to_string()
                }
            }
        "#,
    )?;

    project.cargo_component("build").assert().success();

    let dep = project.debug_wasm("foo");
    validate_component(&dep)?;

    Ok(())
}

#[test]
fn it_builds_with_resources_with_custom_implementor() -> Result<()> {
    let project = Project::new("foo")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    fs::write(
        project.root().join("wit/world.wit"),
        "
            package foo:bar

            world bar {
                export baz: interface {
                    resource keyed-integer {
                        constructor(x: u32)
                        get: func() -> u32
                        set: func(x: u32)
                        key: static func() -> string
                    }
                }
            }
        ",
    )?;

    fs::write(
        project.root().join("src/lib.rs"),
        r#"
            cargo_component_bindings::generate!({
                resources: {
                    "baz/keyed-integer": MyKeyedInteger
                }
            });

            use std::cell::Cell;
            use bindings::exports::baz::GuestKeyedInteger;

            pub struct MyKeyedInteger(Cell<u32>);

            impl GuestKeyedInteger for MyKeyedInteger {
                fn new(x: u32) -> Self {
                    Self(Cell::new(x))
                }

                fn get(&self) -> u32 {
                    self.0.get()
                }

                fn set(&self, x: u32) {
                    self.0.set(x);
                }

                fn key() -> String {
                    "my-key".to_string()
                }
            }
        "#,
    )?;

    project.cargo_component("build").assert().success();

    let dep = project.debug_wasm("foo");
    validate_component(&dep)?;

    Ok(())
}

#[test]
fn it_builds_resources_with_specified_ownership_model() -> Result<()> {
    let project = Project::new("foo")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    fs::write(
        project.root().join("wit/world.wit"),
        "
            package foo:bar

            world bar {
                export baz: interface {
                    resource keyed-integer {
                        constructor(x: u32)
                        get: func() -> u32
                        set: func(x: u32)
                        key: static func() -> string
                    }
                }
            }
        ",
    )?;

    fs::write(
        project.root().join("src/lib.rs"),
        r#"
            cargo_component_bindings::generate!({
                ownership: "borrowing-duplicate-if-necessary"
            });

            use std::cell::Cell;

            pub struct KeyedInteger(Cell<u32>);

            impl bindings::exports::baz::GuestKeyedInteger for KeyedInteger {
                fn new(x: u32) -> Self {
                    Self(Cell::new(x))
                }

                fn get(&self) -> u32 {
                    self.0.get()
                }

                fn set(&self, x: u32) {
                    self.0.set(x);
                }

                fn key() -> String {
                    "my-key".to_string()
                }
            }
        "#,
    )?;

    project.cargo_component("build").assert().success();

    let dep = project.debug_wasm("foo");
    validate_component(&dep)?;

    Ok(())
}

#[test]
fn it_builds_with_a_component_dependency() -> Result<()> {
    let root = create_root()?;

    let comp1 = Project::with_root(&root, "comp1", "")?;
    comp1.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        Ok(doc)
    })?;

    fs::write(
        comp1.root().join("wit/world.wit"),
        "
package my:comp1

interface types {
    record seed {
        value: u32,
    }
}

world random-generator {
    use types.{seed}
    export rand: func(seed: seed) -> u32
}
",
    )?;

    fs::write(
        comp1.root().join("src/lib.rs"),
        r#"
cargo_component_bindings::generate!();

use bindings::{Guest, Seed};

struct Component;

impl Guest for Component {
    fn rand(seed: Seed) -> u32 {
        seed.value + 1
    }
}
"#,
    )?;

    comp1
        .cargo_component("build --release")
        .assert()
        .stderr(contains("Finished release [optimized] target(s)"))
        .success();

    let dep = comp1.release_wasm("comp1");
    validate_component(&dep)?;

    let comp2 = Project::with_root(&root, "comp2", "")?;
    comp2.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        doc["package"]["metadata"]["component"]["dependencies"]["my:comp1"]["path"] =
            value(dep.display().to_string());
        Ok(doc)
    })?;

    fs::write(
        comp2.root().join("wit/world.wit"),
        "
package my:comp2

world random-generator {
    export rand: func() -> u32
}
",
    )?;

    fs::write(
        comp2.root().join("src/lib.rs"),
        r#"
cargo_component_bindings::generate!();

use bindings::{Guest, comp1};

struct Component;

impl Guest for Component {
    fn rand() -> u32 {
        comp1::rand(comp1::Seed { value: 1 })
    }
}
"#,
    )?;

    comp2
        .cargo_component("build --release")
        .assert()
        .stderr(contains("Finished release [optimized] target(s)"))
        .success();

    let path: std::path::PathBuf = comp2.release_wasm("comp2");
    validate_component(&path)?;

    Ok(())
}

#[test]
fn it_builds_with_adapter() -> Result<()> {
    let project = Project::new("foo")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        doc["package"]["metadata"]["component"]["adapter"] = value("not-a-valid-path");
        Ok(doc)
    })?;

    project
        .cargo_component("build")
        .assert()
        .stderr(contains("error: failed to read module adapter"))
        .failure();

    let project = Project::new("foo")?;
    project.update_manifest(|mut doc| {
        redirect_bindings_crate(&mut doc);
        doc["package"]["metadata"]["component"]["adapter"] = value(format!(
            "../../../../../adapters/{version}/wasi_snapshot_preview1.reactor.wasm",
            version = env!("WASI_ADAPTER_VERSION")
        ));
        Ok(doc)
    })?;

    project
        .cargo_component("build")
        .assert()
        .stderr(contains("Finished dev [unoptimized + debuginfo] target(s)"))
        .success();

    validate_component(&project.debug_wasm("foo"))?;

    Ok(())
}
