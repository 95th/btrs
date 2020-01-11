use crate::torrent::TorrentFile;
use bencode::ValueRef;
use reqwest::Client;

pub async fn connect(torrent: &TorrentFile) -> reqwest::Result<()> {
    println!("Announce: {}", torrent.announce);
    println!("InfoHash: {:x?}", torrent.info_hash);
    let url = format!(
        "{}?info_hash={}",
        torrent.announce,
        torrent.info_hash.encode_url()
    );
    let data = Client::new()
        .get(&url)
        .query(&[
            ("peer_id", "-AZ3020-012345678910"),
            ("port", "6881"),
            ("uploaded", "0"),
            ("downloaded", "0"),
            ("compact", "1"),
        ])
        .send()
        .await?
        .bytes()
        .await?;
    let value = ValueRef::decode(&data).unwrap();
    println!("{:x?}", value);
    Ok(())
}
