#[macro_export]
macro_rules! expect_input {
    ($exp:expr, $name:literal, $func:expr) => {
        {
            match $exp {
                Some(input) => $func(input),
                None => {
                    eprintln!("Expected {}", $name);
                    return;
                }
            }
        }
    };
}

// TODO: Convert to procedural macro later
#[macro_export]
macro_rules! impl_str_serde {
    ($name:ident) => {
        impl FromStr for $name {
            type Err = serde_json::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                serde_json::from_str(s)
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", serde_json::to_string(self).unwrap())
            }
        }
    };
}