macro_rules! gbi_command {
    ($name:ident, $process:expr) => {
        pub(crate) fn $name(params: &mut GBICommandParams) -> GBIResult {
            $process(params)
        }
    };
}

pub(crate) use gbi_command;
