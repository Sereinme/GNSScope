pub(crate) mod epoch_dock;
pub(crate) mod overlays;
pub(crate) mod primitives;
pub(crate) mod run_configuration;
pub(crate) mod run_rows;
pub(crate) mod runs_dock;
pub(crate) mod shell;
pub(crate) mod status_strip;
pub(crate) mod title_bar;
pub(crate) mod workbench;

pub(crate) type Localize = fn(&'static str, &'static str) -> &'static str;
