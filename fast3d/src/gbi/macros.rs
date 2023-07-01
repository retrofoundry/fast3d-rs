macro_rules! gbi_command {
    ($name:ident, $process:expr) => {
        pub struct $name;
        impl GBICommand for $name {
            fn process(&self, params: &mut GBICommandParams) -> GBIResult {
                $process(params)
            }
        }
    };
}

pub(crate) use gbi_command;
