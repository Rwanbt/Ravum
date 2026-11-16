use ravum_lang_sema::types::Type;
use std::fmt;

struct BoundedWriter {
    budget: usize,
    text: String,
}

impl fmt::Write for BoundedWriter {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.budget = self.budget.checked_sub(text.len()).ok_or(fmt::Error)?;
        self.text.push_str(text);
        Ok(())
    }
}

#[test]
fn composite_types_propagate_write_errors_at_each_output_boundary() {
    let cases = [
        (Type::Tuple(vec![Type::I32]), "(i32,)"),
        (Type::Tuple(vec![Type::I32, Type::Bool]), "(i32, bool)"),
        (
            Type::Nominal {
                def: 0,
                name: "Pair".to_string(),
                args: vec![Type::I32, Type::Bool],
            },
            "Pair<i32, bool>",
        ),
    ];

    for (ty, expected) in cases {
        for budget in 0..=expected.len() {
            let mut writer = BoundedWriter {
                budget,
                text: String::new(),
            };
            let result = fmt::write(&mut writer, format_args!("{ty}"));

            assert!(expected.starts_with(&writer.text));
            if budget == expected.len() {
                assert_eq!(result, Ok(()));
                assert_eq!(writer.text, expected);
            } else {
                assert_eq!(result, Err(fmt::Error), "output budget {budget} for {ty}");
            }
        }
    }
}
