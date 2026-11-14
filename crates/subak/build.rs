//! Links the program icon and version information into the `subak` binaries (kim-winres).

fn main() {
    kim_winres::embed("../../assets/subak/subak.ico", "Subak");
}
