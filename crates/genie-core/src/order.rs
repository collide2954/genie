const SEX: [&str; 4] = ["X", "Y", "W", "Z"];
const NUMERALS: [(u32, &str); 5] = [(10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")];

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Numbered(u64, String),
    Sex(usize),
    Mitochondrial,
    Other,
}

pub fn contig_order<S: AsRef<str>>(names: &[S]) -> Vec<usize> {
    let roman_ten = names
        .iter()
        .any(|name| roman(bare(name.as_ref())).is_some_and(|value| value > 10));
    let mut order: Vec<usize> = (0..names.len()).collect();
    order.sort_by_cached_key(|&index| rank(bare(names[index].as_ref()), roman_ten));
    order
}

fn bare(name: &str) -> &str {
    match name.get(..3) {
        Some(prefix) if prefix.eq_ignore_ascii_case("chr") && name.len() > 3 => &name[3..],
        _ => name,
    }
}

fn rank(name: &str, roman_ten: bool) -> Rank {
    if let Some(value) = roman(name).filter(|_| name != "X" || roman_ten) {
        return Rank::Numbered(u64::from(value), String::new());
    }
    if let Some(position) = SEX.iter().position(|sex| *sex == name) {
        return Rank::Sex(position);
    }
    if name.eq_ignore_ascii_case("M") || name.eq_ignore_ascii_case("MT") {
        return Rank::Mitochondrial;
    }
    let digits = name.bytes().take_while(u8::is_ascii_digit).count();
    let (number, arm) = name.split_at(digits);
    match number.parse() {
        Ok(number) if arm.bytes().all(|byte| byte.is_ascii_alphabetic()) => {
            Rank::Numbered(number, arm.to_owned())
        }
        _ => Rank::Other,
    }
}

fn roman(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| matches!(byte, b'I' | b'V' | b'X')) {
        return None;
    }
    (1..40).find(|&value| numeral(value) == text)
}

fn numeral(mut value: u32) -> String {
    let mut text = String::new();
    for (step, letters) in NUMERALS {
        while value >= step {
            text.push_str(letters);
            value -= step;
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::contig_order;

    fn sorted<'a>(names: &[&'a str]) -> Vec<&'a str> {
        contig_order(names)
            .into_iter()
            .map(|index| names[index])
            .collect()
    }

    #[test]
    fn human_chromosomes_come_in_number_order_then_x_y_and_m() {
        let t2t = [
            "chr1", "chr2", "chr3", "chr4", "chr5", "chr6", "chr7", "chrX", "chr9", "chr8",
            "chr11", "chr10", "chr12", "chr13", "chr14", "chr15", "chr16", "chr17", "chr18",
            "chr20", "chrY", "chr19", "chr22", "chr21", "chrM",
        ];
        let mut expected: Vec<String> = (1..=22).map(|number| format!("chr{number}")).collect();
        expected.extend(["chrX", "chrY", "chrM"].map(str::to_owned));
        assert_eq!(sorted(&t2t), expected);
    }

    #[test]
    fn other_contigs_follow_in_file_order() {
        let names = [
            "chr1_KI270706v1_random",
            "chr2",
            "chrUn_GL000220v1",
            "chrM",
            "chr1",
            "chrEBV",
            "chr1_KI270707v1_random",
        ];
        assert_eq!(
            sorted(&names),
            [
                "chr1",
                "chr2",
                "chrM",
                "chr1_KI270706v1_random",
                "chrUn_GL000220v1",
                "chrEBV",
                "chr1_KI270707v1_random",
            ]
        );
    }

    #[test]
    fn names_without_chr_sort_the_same_way() {
        let names = ["MT", "X", "10", "2", "GL000192.1", "1", "Y"];
        assert_eq!(
            sorted(&names),
            ["1", "2", "10", "X", "Y", "MT", "GL000192.1"]
        );
    }

    #[test]
    fn fly_arms_follow_their_number() {
        let names = [
            "chrX", "chr3R", "chr2L", "chr4", "chr3L", "chrM", "chr2R", "chrY",
        ];
        assert_eq!(
            sorted(&names),
            [
                "chr2L", "chr2R", "chr3L", "chr3R", "chr4", "chrX", "chrY", "chrM"
            ]
        );
    }

    #[test]
    fn roman_numerals_count_as_numbers() {
        let yeast = [
            "chrIV", "chrXV", "chrVII", "chrXII", "chrXVI", "chrXIII", "chrII", "chrXIV", "chrX",
            "chrXI", "chrV", "chrVIII", "chrIX", "chrIII", "chrVI", "chrI", "chrM",
        ];
        assert_eq!(
            sorted(&yeast),
            [
                "chrI", "chrII", "chrIII", "chrIV", "chrV", "chrVI", "chrVII", "chrVIII", "chrIX",
                "chrX", "chrXI", "chrXII", "chrXIII", "chrXIV", "chrXV", "chrXVI", "chrM",
            ]
        );
    }

    #[test]
    fn x_is_the_sex_chromosome_when_no_numeral_passes_ten() {
        let worm = ["chrV", "chrX", "chrIV", "chrII", "chrM", "chrI", "chrIII"];
        assert_eq!(
            sorted(&worm),
            ["chrI", "chrII", "chrIII", "chrIV", "chrV", "chrX", "chrM"]
        );
    }

    #[test]
    fn files_without_chromosome_names_keep_their_order() {
        let names = ["scaffold_10", "scaffold_2", "contig_a", "chr", "IIII"];
        assert_eq!(sorted(&names), names);
    }
}
