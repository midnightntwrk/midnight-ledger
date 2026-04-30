fn main() {
    use wallet_core::{Network, Wallet};
    for n in Network::ALL {
        let w = Wallet::demo(n);
        println!("{:>11}: {}", n.label(), w.unshielded_address().unwrap());
    }
}
