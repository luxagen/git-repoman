macro_rules! annotated_struct {
    (
        $(#[$attr:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_attr:meta])*
                $field:ident : $ty:ty => $ann:expr
            ),* $(,)?
        }
    ) => {
        $(#[$attr])*
        $vis struct $name {
            $( $(#[$field_attr])* pub $field: $ty ),*
        }

        impl $name {
            pub fn set_by_key(&mut self, key: &str, value: String) {
                match key {
                    $(
                        $ann => {
                            self.$field = value.parse::<$ty>().unwrap_or_default();
                            return;
                        }
                    )*
                    _ => {}
                }
            }

            // AUTO-GENERATED: no manual repetition!
            pub fn all_values(&self) -> Vec<(String, String)> {
                let mut result = Vec::new();
                $(
                    result.push(($ann.to_string(), format!("{}", self.$field)));
                )*
                result
            }
        }
    };
}