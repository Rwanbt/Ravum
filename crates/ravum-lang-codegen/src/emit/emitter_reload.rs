//! Reloadable module entry point (RFC-E11-HOT-RELOAD).
//!
//! Emits one `crate::RvCanon` impl per user type of the state schema and a
//! `main` that hands `init`, `step` and the optional `migrate` to
//! `crate::rv_reload_main` (reload runtime, `prelude::RELOAD_PRELUDE`). The
//! field order of every impl is the schema order of `ravum_lang_sema::reload`,
//! so the canonical text and the digest describe the same layout.

use super::{Buf, Emitter};
use crate::CodegenError;
use ravum_lang_sema::facts::VariantKind;
use ravum_lang_sema::reload::ReloadContract;
use ravum_lang_sema::types::Type;
use ravum_lang_syntax::ast::{Module, NodeId};

impl Emitter<'_> {
    /// Crate `main`: delegate to the entry emitted inside the root module,
    /// where the private root items are visible.
    pub(super) fn emit_reload_main(
        &self,
        buf: &mut Buf,
        _contract: &ReloadContract,
    ) -> Result<(), CodegenError> {
        let root = &self.index.modules[&self.checked.program.root.id];
        buf.line("fn main() -> std::process::ExitCode {");
        buf.indent += 1;
        buf.line(&format!("crate::{}::rv_reload_entry()", root.rust_name));
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }

    /// Inside `module`: the canonical impls of the state types it defines
    /// (their private fields are visible here) and, in the root module, the
    /// reload entry point.
    pub(super) fn emit_reload_items(
        &self,
        buf: &mut Buf,
        module: &Module,
        contract: &ReloadContract,
    ) -> Result<(), CodegenError> {
        let types = contract.serialized_types();
        for item in &module.items {
            if types.contains(&item.id) {
                buf.blank();
                self.emit_canon_impl(buf, item.id)?;
            }
        }
        if module.id != self.checked.program.root.id {
            return Ok(());
        }
        buf.blank();
        let state = self.type_path(contract.state.def)?;
        let (old, old_schema, migrate) = match &contract.migrate {
            Some((id, schema)) => (
                self.type_path(schema.def)?,
                format!("Some(\"{}\")", schema.digest),
                format!("Some({})", self.item_path(*id)?),
            ),
            None => ("()".to_string(), "None".to_string(), "None".to_string()),
        };
        buf.line("pub(crate) fn rv_reload_entry() -> std::process::ExitCode {");
        buf.indent += 1;
        buf.line(&format!(
            "crate::rv_reload_main::<{state}, {old}>(\"{}\", {old_schema}, {}, {}, {migrate})",
            contract.state.digest,
            self.item_path(contract.init)?,
            self.item_path(contract.step)?,
        ));
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }

    fn type_path(&self, def: NodeId) -> Result<String, CodegenError> {
        let facts = &self.checked.facts;
        let name = facts
            .structs
            .get(&def)
            .map(|s| s.name.clone())
            .or_else(|| facts.enums.get(&def).map(|e| e.name.clone()))
            .unwrap_or_default();
        self.nominal_path(def, &name)
    }

    fn get_expr(&self, ty: &Type) -> Result<String, CodegenError> {
        Ok(format!(
            "{{ let rv_item = <{} as crate::RvCanon>::rv_get(input)?; input.expect(';')?; rv_item }}",
            self.fmt_type(ty)?
        ))
    }

    fn emit_canon_impl(&self, buf: &mut Buf, def: NodeId) -> Result<(), CodegenError> {
        let path = self.type_path(def)?;
        let facts = &self.checked.facts;
        buf.line(&format!("impl crate::RvCanon for {path} {{"));
        buf.indent += 1;
        if let Some(info) = facts.structs.get(&def) {
            buf.line("fn rv_put(&self, out: &mut String) {");
            buf.indent += 1;
            buf.line("out.push('{');");
            for field in &info.fields {
                buf.line(&format!(
                    "crate::RvCanon::rv_put(&self.{}, out); out.push(';');",
                    field.rust_name
                ));
            }
            buf.line("out.push('}');");
            buf.indent -= 1;
            buf.line("}");
            buf.line("fn rv_get(input: &mut crate::RvReader) -> Result<Self, String> {");
            buf.indent += 1;
            buf.line("input.expect('{')?;");
            buf.line(&format!("let rv_value = {path} {{"));
            buf.indent += 1;
            for field in &info.fields {
                buf.line(&format!(
                    "{}: {},",
                    field.rust_name,
                    self.get_expr(&field.ty)?
                ));
            }
            buf.indent -= 1;
            buf.line("};");
            buf.line("input.expect('}')?;");
            buf.line("Ok(rv_value)");
            buf.indent -= 1;
            buf.line("}");
        } else if let Some(info) = facts.enums.get(&def) {
            buf.line("fn rv_put(&self, out: &mut String) {");
            buf.indent += 1;
            buf.line("match self {");
            buf.indent += 1;
            for variant in &info.variants {
                let (pattern, puts) = match variant.kind {
                    VariantKind::Unit => (String::new(), Vec::new()),
                    VariantKind::Tuple => {
                        let names: Vec<String> = (0..variant.tuple_types.len())
                            .map(|i| format!("rv_f{i}"))
                            .collect();
                        (format!("({})", names.join(", ")), names)
                    }
                    VariantKind::Struct => {
                        let names: Vec<String> =
                            variant.fields.iter().map(|f| f.rust_name.clone()).collect();
                        (format!(" {{ {} }}", names.join(", ")), names)
                    }
                };
                buf.line(&format!("{path}::{}{pattern} => {{", variant.rust_name));
                buf.indent += 1;
                buf.line(&format!("out.push_str(\"{}(\");", variant.index));
                for name in puts {
                    buf.line(&format!(
                        "crate::RvCanon::rv_put({name}, out); out.push(';');"
                    ));
                }
                buf.line("out.push(')');");
                buf.indent -= 1;
                buf.line("}");
            }
            buf.indent -= 1;
            buf.line("}");
            buf.indent -= 1;
            buf.line("}");
            buf.line("fn rv_get(input: &mut crate::RvReader) -> Result<Self, String> {");
            buf.indent += 1;
            buf.line("let rv_value = match crate::rv_get_tag(input)? {");
            buf.indent += 1;
            for variant in &info.variants {
                let built = match variant.kind {
                    VariantKind::Unit => format!("{path}::{}", variant.rust_name),
                    VariantKind::Tuple => {
                        let items = variant
                            .tuple_types
                            .iter()
                            .map(|ty| self.get_expr(ty))
                            .collect::<Result<Vec<_>, _>>()?;
                        format!("{path}::{}({})", variant.rust_name, items.join(", "))
                    }
                    VariantKind::Struct => {
                        let items = variant
                            .fields
                            .iter()
                            .map(|f| Ok(format!("{}: {}", f.rust_name, self.get_expr(&f.ty)?)))
                            .collect::<Result<Vec<_>, CodegenError>>()?;
                        format!("{path}::{} {{ {} }}", variant.rust_name, items.join(", "))
                    }
                };
                buf.line(&format!("{} => {built},", variant.index));
            }
            buf.line("other => return Err(format!(\"invalid variant tag {}\", other)),");
            buf.indent -= 1;
            buf.line("};");
            buf.line("input.expect(')')?;");
            buf.line("Ok(rv_value)");
            buf.indent -= 1;
            buf.line("}");
        }
        buf.indent -= 1;
        buf.line("}");
        Ok(())
    }
}

impl Emitter<'_> {
    /// The DSP block ABI (`ravum-protocol/src/dsp.rs`, DSP_ABI_VERSION 2) around
    /// the user's `process_sample`, emitted inside the root module.
    ///
    /// This is the only `unsafe` of a DSP block (DSP-1.3): one module, written by
    /// the generator, never by the user. It turns the host's raw block pointers
    /// into slices, copies the host state into the Ravum `State` (same schema,
    /// checked by `dsp_contract` and by the host), runs one call per sample and
    /// writes the state back. It allocates nothing.
    pub(super) fn emit_dsp_wrapper(
        &self,
        buf: &mut Buf,
        contract: &ravum_lang_sema::reload::DspContract,
    ) -> Result<(), CodegenError> {
        if contract.abi == 3 {
            return self.emit_dsp3_wrapper(buf, contract);
        }
        let state = self.type_path(contract.state.def)?;
        let process = self.item_path(contract.process)?;
        let fields: Vec<String> = self.checked.facts.structs[&contract.state.def]
            .fields
            .iter()
            .map(|f| f.rust_name.clone())
            .collect();
        let manifest = format!(
            "{{\\\"schema_version\\\":1,\\\"patch_id\\\":\\\"{}\\\",\\\"export\\\":\\\"ravum_process_block\\\",\\\"signature\\\":\\\"block:f32->f32+state:v1\\\",\\\"author\\\":\\\"ravum-lang\\\"}}",
            self.options.crate_name
        );
        let lines = [
            String::new(),
            "#[allow(unsafe_code)]".to_string(),
            "pub mod rv_dsp_abi {".to_string(),
            "    #[repr(C)]".to_string(),
            "    pub struct RvHostState {".to_string(),
            "        pub z1: f32,".to_string(),
            "        pub z2: f32,".to_string(),
            "    }".to_string(),
            format!("    const MANIFEST: &[u8] = b\"{manifest}\\0\";"),
            format!("    const STATE_SCHEMA: &[u8] = b\"{}\\0\";", contract.state.digest),
            "    #[no_mangle]".to_string(),
            "    pub extern \"C\" fn ravum_abi_version() -> u32 {".to_string(),
            "        2".to_string(),
            "    }".to_string(),
            "    #[no_mangle]".to_string(),
            "    pub extern \"C\" fn ravum_manifest_v1() -> *const std::os::raw::c_char {".to_string(),
            "        MANIFEST.as_ptr() as *const std::os::raw::c_char".to_string(),
            "    }".to_string(),
            "    #[no_mangle]".to_string(),
            "    pub extern \"C\" fn ravum_state_schema() -> *const std::os::raw::c_char {".to_string(),
            "        STATE_SCHEMA.as_ptr() as *const std::os::raw::c_char".to_string(),
            "    }".to_string(),
            "    /// # Safety".to_string(),
            "    /// `input` and `output` hold `frames` samples, `state` is a valid".to_string(),
            "    /// host state; none is retained after the call.".to_string(),
            "    #[no_mangle]".to_string(),
            "    pub unsafe extern \"C\" fn ravum_process_block(input: *const f32, output: *mut f32, frames: u32, state: *mut RvHostState) {".to_string(),
            "        if input.is_null() || output.is_null() || state.is_null() || frames == 0 {".to_string(),
            "            return;".to_string(),
            "        }".to_string(),
            "        let n = frames as usize;".to_string(),
            "        // SAFETY: the host passes valid, non-overlapping buffers of `n` samples.".to_string(),
            "        let input = unsafe { std::slice::from_raw_parts(input, n) };".to_string(),
            "        let output = unsafe { std::slice::from_raw_parts_mut(output, n) };".to_string(),
            "        // SAFETY: the host owns the state and passes it for this call only.".to_string(),
            "        let host = unsafe { &mut *state };".to_string(),
            format!(
                "        let mut rv_state = {state} {{ {} }};",
                fields.iter().map(|f| format!("{f}: host.{f}")).collect::<Vec<_>>().join(", ")
            ),
            "        for (x, y) in input.iter().zip(output.iter_mut()) {".to_string(),
            format!("            *y = {process}(&mut rv_state, *x);"),
            "        }".to_string(),
            format!(
                "        {}",
                fields.iter().map(|f| format!("host.{f} = rv_state.{f};")).collect::<Vec<_>>().join(" ")
            ),
            "    }".to_string(),
            "}".to_string(),
        ];
        for line in lines {
            buf.line(&line);
        }
        Ok(())
    }

    /// RFC-DSP-ABI-2 (ABI 3): the state is the module's own `repr(C)` `State`,
    /// held by the host in an opaque buffer of `ravum_state_layout()` bytes
    /// and initialised by `ravum_state_init`. Same confinement as ABI 2: one
    /// generated module, never written by the user (DSP-1.3).
    fn emit_dsp3_wrapper(
        &self,
        buf: &mut Buf,
        contract: &ravum_lang_sema::reload::DspContract,
    ) -> Result<(), CodegenError> {
        let state = self.type_path(contract.state.def)?;
        let process = self.item_path(contract.process)?;
        let init = match contract.init {
            Some(init) => self.item_path(init)?,
            None => {
                return Err(super::err(
                    "an ABI 3 DSP block has no `init` (checked by dsp_contract)",
                ))
            }
        };
        let manifest = format!(
            "{{\\\"schema_version\\\":1,\\\"patch_id\\\":\\\"{}\\\",\\\"export\\\":\\\"ravum_process_block\\\",\\\"signature\\\":\\\"block:f32->f32+state:opaque:v3\\\",\\\"author\\\":\\\"ravum-lang\\\"}}",
            self.options.crate_name
        );
        let lines = [
            String::new(),
            "#[allow(unsafe_code)]".to_string(),
            "pub mod rv_dsp_abi {".to_string(),
            "    #[repr(C)]".to_string(),
            "    pub struct RvStateLayout {".to_string(),
            "        pub size: u32,".to_string(),
            "        pub align: u32,".to_string(),
            "    }".to_string(),
            format!("    const MANIFEST: &[u8] = b\"{manifest}\\0\";"),
            format!("    const STATE_SCHEMA: &[u8] = b\"{}\\0\";", contract.state.digest),
            "    #[no_mangle]".to_string(),
            "    pub extern \"C\" fn ravum_abi_version() -> u32 {".to_string(),
            "        3".to_string(),
            "    }".to_string(),
            "    #[no_mangle]".to_string(),
            "    pub extern \"C\" fn ravum_manifest_v1() -> *const std::os::raw::c_char {".to_string(),
            "        MANIFEST.as_ptr() as *const std::os::raw::c_char".to_string(),
            "    }".to_string(),
            "    #[no_mangle]".to_string(),
            "    pub extern \"C\" fn ravum_state_schema() -> *const std::os::raw::c_char {".to_string(),
            "        STATE_SCHEMA.as_ptr() as *const std::os::raw::c_char".to_string(),
            "    }".to_string(),
            "    #[no_mangle]".to_string(),
            "    pub extern \"C\" fn ravum_state_layout() -> RvStateLayout {".to_string(),
            "        RvStateLayout {".to_string(),
            format!("            size: std::mem::size_of::<{state}>() as u32,"),
            format!("            align: std::mem::align_of::<{state}>() as u32,"),
            "        }".to_string(),
            "    }".to_string(),
            "    /// # Safety".to_string(),
            "    /// `state` points to `size` writable bytes aligned to `align`.".to_string(),
            "    #[no_mangle]".to_string(),
            "    pub unsafe extern \"C\" fn ravum_state_init(state: *mut u8) {".to_string(),
            "        if state.is_null() {".to_string(),
            "            return;".to_string(),
            "        }".to_string(),
            "        // SAFETY: the host allocated the buffer with this module's layout.".to_string(),
            format!("        unsafe {{ std::ptr::write(state as *mut {state}, {init}()) }};"),
            "    }".to_string(),
            "    /// # Safety".to_string(),
            "    /// `input` and `output` hold `frames` samples; `state` was initialised".to_string(),
            "    /// by `ravum_state_init` of a module with the same schema digest.".to_string(),
            "    #[no_mangle]".to_string(),
            "    pub unsafe extern \"C\" fn ravum_process_block(input: *const f32, output: *mut f32, frames: u32, state: *mut u8) {".to_string(),
            "        if input.is_null() || output.is_null() || state.is_null() || frames == 0 {".to_string(),
            "            return;".to_string(),
            "        }".to_string(),
            "        let n = frames as usize;".to_string(),
            "        // SAFETY: the host passes valid, non-overlapping buffers of `n` samples.".to_string(),
            "        let input = unsafe { std::slice::from_raw_parts(input, n) };".to_string(),
            "        let output = unsafe { std::slice::from_raw_parts_mut(output, n) };".to_string(),
            "        // SAFETY: an initialised `State` of this schema, owned by the host.".to_string(),
            format!("        let rv_state = unsafe {{ &mut *(state as *mut {state}) }};"),
            "        for (x, y) in input.iter().zip(output.iter_mut()) {".to_string(),
            format!("            *y = {process}(rv_state, *x);"),
            "        }".to_string(),
            "    }".to_string(),
            "}".to_string(),
        ];
        for line in lines {
            buf.line(&line);
        }
        // DSP3-5: the migration the host may run, on its thread, from a state
        // of schema `ravum_state_migrate_from` to this module's state.
        if let Some((migrate, old)) = &contract.migrate {
            let migrate_path = self.item_path(*migrate)?;
            let old_ty = self.type_path(old.def)?;
            let extra = [
                "#[allow(unsafe_code)]".to_string(),
                "pub mod rv_dsp_migrate {".to_string(),
                format!("    const FROM: &[u8] = b\"{}\\0\";", old.digest),
                "    #[no_mangle]".to_string(),
                "    pub extern \"C\" fn ravum_state_migrate_from() -> *const std::os::raw::c_char {".to_string(),
                "        FROM.as_ptr() as *const std::os::raw::c_char".to_string(),
                "    }".to_string(),
                "    /// # Safety".to_string(),
                "    /// `old` is an initialised state of schema `FROM`; `new` has this".to_string(),
                "    /// module's state layout; the two do not overlap.".to_string(),
                "    #[no_mangle]".to_string(),
                "    pub unsafe extern \"C\" fn ravum_state_migrate(old: *const u8, new: *mut u8) {".to_string(),
                "        if old.is_null() || new.is_null() {".to_string(),
                "            return;".to_string(),
                "        }".to_string(),
                "        // SAFETY: `repr(C)` types of this module, buffers given by the host.".to_string(),
                format!("        let value = unsafe {{ std::ptr::read(old as *const {old_ty}) }};"),
                format!("        unsafe {{ std::ptr::write(new as *mut {}, {migrate_path}(value)) }};", self.type_path(contract.state.def)?),
                "    }".to_string(),
                "}".to_string(),
            ];
            for line in extra {
                buf.line(&line);
            }
        }
        Ok(())
    }
}
