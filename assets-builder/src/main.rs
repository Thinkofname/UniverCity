extern crate byteorder;

use byteorder::{LittleEndian, WriteBytesExt};
use std::env;
use std::fs;
use std::io::{self, Seek, Write};
use std::path::{self, Path, PathBuf};

fn main() {
    let mut args = env::args();
    args.next();
    let pack = args.next().unwrap();
    build_assets(pack);
}

fn build_assets(name: String) {
    let mut assets = vec![];
    let root = format!("./assets/{}", name);
    collect_files(&mut assets, &root);

    fs::create_dir_all("./assets/packed/").unwrap();

    let mut pck = fs::File::create(&format!("./assets/packed/{}.assets", name)).unwrap();
    let mut pck_index = fs::File::create(&format!("./assets/packed/{}.index", name)).unwrap();
    pck_index
        .write_u32::<LittleEndian>(assets.len() as u32)
        .unwrap();
    for asset in assets {
        let asset_short = asset.strip_prefix(&root).unwrap();
        let mut standard_path = String::new();
        for part in asset_short.components() {
            standard_path.push_str("/");
            if let path::Component::Normal(p) = part {
                standard_path.push_str(p.to_str().unwrap());
            } else {
                break;
            }
        }
        let mut data = fs::File::open(&asset).unwrap();
        let start = pck.seek(io::SeekFrom::Current(0)).unwrap();
        let len = io::copy(&mut data, &mut pck).unwrap();

        pck_index
            .write_u16::<LittleEndian>(standard_path.len() as u16)
            .unwrap();
        pck_index.write_all(standard_path.as_bytes()).unwrap();
        pck_index.write_u64::<LittleEndian>(start).unwrap();
        pck_index.write_u64::<LittleEndian>(len).unwrap();
    }
}

fn collect_files<P: AsRef<Path>>(assets: &mut Vec<PathBuf>, path: P) {
    let files = fs::read_dir(path).unwrap().into_iter().map(|v| v.unwrap());
    for file in files {
        if file.file_type().unwrap().is_dir() {
            collect_files(assets, file.path());
        } else {
            assets.push(file.path());
        }
    }
}
