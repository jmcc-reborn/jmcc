//! Генерирует `generated.rs` из JSON-схемы `JustMC` (`assets/*.json`).
//!
//! Результат пишется в `OUT_DIR/generated.rs` и подключается через `include!`
//! из `src/generated.rs`. Править сгенерированный файл руками нельзя.
//!
//! Скрипт разложен по ответственностям: `build/assets.rs` читает схему,
//! `build/{enums,actions,op_builder,lookups}.rs` пишут по одному артефакту,
//! `build/library.rs` собирает их в один `TokenStream`, а `main` задаёт порядок
//! и пишет файл.
//!
//! `build.rs` — корень этого крейта, поэтому `mod foo;` искал бы `foo` рядом с
//! `build.rs`; модули из подкаталога подключаются через `#[path]`.

#[path = "build/actions.rs"]
mod actions;
#[path = "build/assets.rs"]
mod assets;
#[path = "build/enums.rs"]
mod enums;
#[path = "build/library.rs"]
mod library;
#[path = "build/lookups.rs"]
mod lookups;
#[path = "build/op_builder.rs"]
mod op_builder;
#[path = "build/util.rs"]
mod util;

use std::error::Error;

use crate::assets::load_assets;
use crate::library::gen_library;
use crate::lookups::gen_lookup_maps;
use crate::util::write_to_out_dir;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=assets");

    let assets = load_assets()?;

    let mut output = gen_library(&assets).to_string();
    output.push('\n');
    output.push_str(&gen_lookup_maps(&assets));

    write_to_out_dir("generated.rs", &output)?;

    Ok(())
}
