/// Define a GBI command macro.
///
/// This macro simplifies the creation of GBI command functions.
/// It takes a name and a processing expression and generates a function with the specified name.
///
/// # Example
///
/// ```rust ignore
/// gbi_command!(my_command, |params| {
///     // processing logic here
/// });
/// ```
macro_rules! gbi_command {
    ($name:ident, $process:expr) => {
        #[allow(non_snake_case)]
        pub(crate) fn $name(params: &mut GBICommandParams) -> GBIResult {
            let process_closure = $process;
            process_closure(params)
        }
    };
}

pub(crate) use gbi_command;
