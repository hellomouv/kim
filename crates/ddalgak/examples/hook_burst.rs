//! Step 82 (`cargo xtask bench-ddalgak`): sends `count` taps of F20 as another program, spaced by
//! `gap_ms`, so a running (test-host) hook measures its callback time. F20 is an obscure key no
//! Kim tool binds; nothing else is sent. Prints how many events were inserted (0 on a locked
//! desktop, where SendInput is refused).

fn main() {
    let mut args = std::env::args().skip(1);
    let count: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(200);
    let gap: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(5);
    #[cfg(windows)]
    {
        use ddalgak::inject::OutKey;
        const VK_F20: u16 = 0x83;
        let mut sent = 0;
        for _ in 0..count.min(2000) {
            sent += ddalgak::win::inject::send_as_other_program(&[
                OutKey {
                    vk: VK_F20,
                    up: false,
                },
                OutKey {
                    vk: VK_F20,
                    up: true,
                },
            ]);
            std::thread::sleep(std::time::Duration::from_millis(gap));
        }
        println!("{sent}");
    }
    #[cfg(not(windows))]
    {
        let _ = (count, gap);
        println!("0");
    }
}
