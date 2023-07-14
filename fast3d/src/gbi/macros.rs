macro_rules! gbi_command {
    ($name:ident, $process:expr) => {
        #[allow(non_snake_case)]
        pub(crate) fn $name(params: &mut GBICommandParams) -> GBIResult {
            $process(params)
        }
    };
}

pub(crate) use gbi_command;
