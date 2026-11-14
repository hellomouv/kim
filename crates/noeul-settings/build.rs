//! Links the program icon and version information into `noeul-settings.exe` (kim-winres).

fn main() {
    kim_winres::embed("../../assets/noeul/noeul.ico", "Noeul settings");
}
