use osu_db::{listing::Listing, score::ScoreList};
use rosu_pp::{Beatmap, BeatmapExt};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Write},
    path::PathBuf,
};
use chrono::Local;
use bitflags::bitflags;

#[derive(Clone)]
struct ScoreData {
    score: osu_db::Replay,
    map: osu_db::listing::Beatmap,
    attributes: rosu_pp::PerformanceAttributes,
}

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    println!("Reading scores.db");
    let score_list = ScoreList::from_file("scores.db")
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    println!("Scores.db found with {} beatmaps", score_list.beatmaps.len());

    println!("Reading osu!.db");
    let listing = Listing::from_file("osu!.db")
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    println!("osu!.db found with {} beatmaps", listing.beatmaps.len());

    println!("Processing maps and scores with pp calc");
    let scores_with_pp = process_scores(&score_list, &listing);
    println!("Processed {} scores", scores_with_pp.len());

    println!("Removing duplicates through pp sort");
    let mut unique_scores = remove_duplicates(scores_with_pp);
    println!("Down to {} scores", unique_scores.len());
    
    println!("Exporting tops to .txt");
    export_tops(&mut unique_scores)?;
    print!("Done");

    Ok(())
}

fn process_scores(score_list: &ScoreList, listing: &Listing) -> Vec<ScoreData> {
    let mut scores_with_pp = Vec::new();
    let mut map_count = 0;

    for beatmap_scores in &score_list.beatmaps {
        println!("Processing beatmap in database {}/{}", map_count, score_list.beatmaps.len());
        let mut score_count = 0;
        for score in &beatmap_scores.scores {
            println!("Processing score in beatmap {}/{}", score_count, beatmap_scores.scores.len());
            let beatmap = match listing
                .beatmaps
                .iter()
                .find(|b| b.hash == score.beatmap_hash) {
                    Some(beatmap) => beatmap,
                    None => {
                        eprintln!("Error: Beatmap not found");
                        continue;
                    }
                };
            
            if beatmap.status == osu_db::listing::RankedStatus::Ranked {continue;}

            let path = PathBuf::from("Songs")
                .join(&beatmap.folder_name.as_ref().unwrap_or(&"Unknown Folder".to_string()))
                .join(&beatmap.file_name.as_ref().unwrap_or(&"Unknown File".to_string()));

            let map = match Beatmap::from_path(&path) {
                Ok(map) => map,
                Err(e) => {
                    eprintln!("Error: {}", e.to_string());
                    continue;
                }
            };

            let attributes = map
                .pp()
                .mods(score.mods.0)
                .combo(score.max_combo as usize)
                .n_misses(score.count_miss as usize)
                .n300(score.count_300 as usize)
                .n100(score.count_100 as usize)
                .n50(score.count_50 as usize)
                .calculate();

            if attributes.pp() >= 2000.0 {continue;}

            scores_with_pp.push(ScoreData {
                score: score.clone(),
                map: beatmap.clone(),
                attributes: attributes.clone(),
            });
            score_count += 1;
        }
        map_count += 1;
    }
    scores_with_pp
}

fn remove_duplicates(scores_with_pp: Vec<ScoreData>) -> Vec<ScoreData> {
    let mut scores_by_hash: HashMap<String, ScoreData> = HashMap::new();
    for score_pp in scores_with_pp {
        let hash = score_pp
            .score
            .beatmap_hash
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        scores_by_hash
            .entry(hash)
            .and_modify(|e| {
                if score_pp.attributes.pp() > e.attributes.pp() {
                    *e = score_pp.clone();
                }
            })
            .or_insert(score_pp.clone());
    }

    scores_by_hash.into_values().collect()
}

// https://github.com/ppy/osu-api/wiki#mods
bitflags! {
    #[derive(Debug)]
    struct Mods: u32 {
        const NoMod           = 0;
        const NoFail         = 1 << 0;
        const Easy           = 1 << 1;
        const TouchDevice    = 1 << 2;
        const Hidden         = 1 << 3;
        const HardRock       = 1 << 4;
        const SuddenDeath    = 1 << 5;
        const DoubleTime     = 1 << 6;
        const Relax          = 1 << 7;
        const HalfTime       = 1 << 8;
        const Nightcore      = 1 << 9; // Only set along with DoubleTime. i.e: NC only gives 576
        const Flashlight     = 1 << 10;
        const Autoplay       = 1 << 11;
        const SpunOut        = 1 << 12;
        const Relax2         = 1 << 13; // Autopilot
        const Perfect        = 1 << 14; // Only set along with SuddenDeath. i.e: PF only gives 16416
        const Key4           = 1 << 15;
        const Key5           = 1 << 16;
        const Key6           = 1 << 17;
        const Key7           = 1 << 18;
        const Key8           = 1 << 19;
        const FadeIn         = 1 << 20;
        const Random         = 1 << 21;
        const Cinema         = 1 << 22;
        const Target         = 1 << 23;
        const Key9           = 1 << 24;
        const KeyCoop        = 1 << 25;
        const Key1           = 1 << 26;
        const Key3           = 1 << 27;
        const Key2           = 1 << 28;
        const ScoreV2        = 1 << 29;
        const Mirror         = 1 << 30;
    }
}

fn export_tops(unique_scores: &mut [ScoreData]) -> Result<(), io::Error> {
    unique_scores.sort_by(|a, b| b.attributes.pp().partial_cmp(&a.attributes.pp()).unwrap_or(std::cmp::Ordering::Equal));

    let timestamp = Local::now().format("%Y-%m-%dT%H-%M-%S").to_string();
    let file_name = format!("tops_{}.txt", timestamp);
    let mut file = File::create(&file_name)?;
    println!("{} initialized", file_name);

    println!("Calculating total pp (without bonus pp)");
    // https://osu.ppy.sh/wiki/en/Performance_points/Weighting_system
    let total_pp: f64 = unique_scores
        .iter()
        .enumerate()
        .map(|(i, score_pp)| score_pp.attributes.pp() * 0.95f64.powi(i as i32))
        .sum();
    println!("Total pp (without bonus pp): {:.2}", total_pp);

    println!("Calculating bonus pp");
    let bonus_pp: f64 = (417.0 - 1.0 / 3.0) * (1.0 - 0.995f64.powf(std::cmp::min(1000, unique_scores.len()) as f64));
    println!("Bonus pp: {:.2}", bonus_pp);

    println!("Counting 9* PFCs");
    let count = unique_scores.iter()
        .filter(|score| score.attributes.stars() >= 9.0 && score.attributes.stars() < 10.0 && score.score.perfect_combo)
        .count();
    println!("9* PFCs: {}", count);

    writeln!(file, "Total pp: {:.2}", total_pp + bonus_pp)?;
    writeln!(file, "Total pp (without bonus pp): {:.2}", total_pp)?;
    writeln!(file, "Bonus pp: {:.2}", bonus_pp)?;
    writeln!(file, "9* PFCs: {}", count)?;

    println!("Writing top 100");
    for (i, score_pp) in unique_scores.iter().take(100).enumerate() {
        println!("Writing top {}/{}", i, 100);
        let mods = Mods::from_bits(score_pp.score.mods.0).unwrap_or(Mods::NoMod);
        let mod_display = if mods.is_empty() {
            "NoMod".to_string()
        } else {
            format!("{:?}", mods)
        };
    
        writeln!(
            file,
            "{:3}. {}\t{} [{}]",
            i + 1,
            score_pp.map.artist_ascii.as_ref().unwrap_or(&"Unknown Artist".to_string()),
            score_pp.map.title_ascii.as_ref().unwrap_or(&"Unknown Title".to_string()),
            score_pp.map.difficulty_name.as_ref().unwrap_or(&"Unknown Difficulty".to_string())
        )?;

        writeln!(
            file,
            "     {:.2}pp {}",
            score_pp.attributes.pp(),
            mod_display
        )?;
    }
    println!("Top 100 scores written");

    Ok(())
}