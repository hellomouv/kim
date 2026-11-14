//! Links the program icon and version information into the `ddalgak` binaries (kim-winres).

fn main() {
    kim_winres::embed("../../assets/ddalgak/ddalgak.ico", "Ddalgak");
}
