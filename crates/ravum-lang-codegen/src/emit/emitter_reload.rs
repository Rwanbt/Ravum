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
