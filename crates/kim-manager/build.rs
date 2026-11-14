//! Links the program icon and version information into the `kim-manager` binaries (kim-winres).

fn main() {
    kim_winres::embed("../../assets/icon/kim.ico", "Kim Manager");
}
