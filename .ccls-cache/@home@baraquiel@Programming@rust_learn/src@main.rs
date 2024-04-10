use regex::Regex;
use std::fs;

fn main() {
    let re = Regex::new(r"(?m)(^|\n)([#]+)(( |\t)+)(.+)$").unwrap();

    //let src = fs::read_to_string("./readme.nml").unwrap();
    let src = String::from("# Test\n## Second line!\n### Third");

    let mut result = Vec::<String>::new();
    for (line, [_, count, spacing, string]) in re.captures_iter(&src).map(|v| v.extract())
    {
        println!("`{line}`:\n{count}/{spacing}/{string}\n");
    }
    //test(&p);

    /*
    */
    //t1.join().unwrap();
    //t2.join().unwrap();

}
