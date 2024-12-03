#[macro_export]
macro_rules! expect_input {
    ($exp:expr, $name:literal, $func:expr) => {{
        match $exp {
            Some(input) => $func(input),
            None => {
                eprintln!("Expected {}", $name);
                return;
            }
        }
    }};
}

// TODO: Convert to procedural macro later
#[macro_export]
macro_rules! impl_str_serde {
    ($name:ident) => {
        impl std::str::FromStr for $name {
            type Err = serde_json::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if !s.starts_with("\"") {
                    serde_json::from_str(format!("\"{s}\"").as_str())
                } else {
                    serde_json::from_str(s)
                }
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                let s = serde_json::to_string(self).unwrap();
                write!(f, "{}", &s[1..(s.len() - 1)])
            }
        }
    };
}
