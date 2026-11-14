//! Links the program icon and version information into the `noeul` binaries (kim-winres).

fn main() {
    kim_winres::embed("../../assets/noeul/noeul.ico", "Noeul");
}
