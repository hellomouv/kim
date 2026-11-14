//! Links the program icon and version information into the `ddalgak-editor` binaries (kim-winres).

fn main() {
    kim_winres::embed("../../assets/ddalgak/ddalgak.ico", "Ddalgak rule editor");
}
