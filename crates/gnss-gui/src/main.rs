use gnss_gui::{render_workspace_snapshot, GuiReceiverWorkspaceState};

pub fn main() {
    let state = GuiReceiverWorkspaceState::preview();
    print!("{}", render_workspace_snapshot(&state));
}
