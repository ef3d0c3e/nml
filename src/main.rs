mod parser;
use self::parser::rule::SyntaxRule;
use self::parser::section::SectionRule;
mod files;
use self::files::file::File;
use self::files::cursor::Cursor;
mod syntax;
use syntax::element::Element;
use syntax::element::Text;

fn main() {
    let file = File::new(String::from("./test.nml"));
    let mut cursor = Cursor::new(&file).unwrap();
    cursor.position = 5;

    let rule_se = SectionRule::new();
    let (token, res) = rule_se.on_match(&cursor).unwrap();
    println!("{}", res.elements.len());


    /*
    let re_sections = regex::Regex::new(r"(?:^|\n)(#{1,})(\*|\+)((?:\t| ){0,})(.*)").unwrap();

    //let mut validators = Vec::<Box<dyn GroupValidator>>::new();
    let f = File::new(Box::new(std::path::Path::new("./test.nml")));
    let content = std::fs::read_to_string(*f.path).unwrap();
    
    let grammar = vec![re_sections];
    let mut positions = [0usize; 1];

    let mut i = 0;
    while i < content.len()
    {
        // Update every positions
        for k in 0..grammar.len()
        {
            let rule = &grammar[k];
            let position = &mut positions[k];
            if *position == std::usize::MAX { continue };

            match rule.find_at(&content, i)
            {
                Some(mat) => *position = mat.start(),
                None => *position = std::usize::MAX,
            }
            println!("{position}");
        }

        // Gets closest match
        let mut next_position = std::usize::MAX;
        let mut closest_match = std::usize::MAX;
        for k in 0..grammar.len()
        {
            if positions[k] >= next_position { continue; }

            next_position = positions[k];
            closest_match = k;
        }

        println!("Unmatched: {}", &content[i..next_position]);

        // No matches left
        if closest_match == std::usize::MAX
        {
            println!("Done");
            break;
        }

        // Extract matches from rule
        i = next_position; // Set to begining of match
        let mat = &grammar[closest_match].captures_at(&content, i).unwrap(); // Capture match
        for m in 0..mat.len()
        {
            match mat.get(m)
            {
                Some(s) => { 
                    println!("Group {m}: `{}`", s.as_str());
                },
                None => println!("Group {m}: None"),
            }
        }

        i += mat.get(0).unwrap().len(); // Add match length
        println!("Left={}", &content[i..]);
        println!("pos={i}");

        let mut s = String::new();
        std::io::stdin().read_line(&mut s).expect("Did not enter a correct string");
    }
    */
    

    
    /*
    validators.push(Box::new(StringValidator::new("Depth".to_string(), |_group| -> ValidationStatus {
        ValidationStatus::Ok()
    })));
    validators.push(Box::new(StringValidator::new("Index Type".to_string(), |group| -> ValidationStatus {
        match group
        {
            "" => ValidationStatus::Ok(),
            "*" => ValidationStatus::Ok(),
            _ => ValidationStatus::Error("")
        }
        ValidationStatus::Ok()
    })));
    */
    //let _sec_rule = SyntaxRule::new("Section".to_string(), r"(?m)(?:^|\n)(#{1,})(\\*|\\+)((?:\t| ){0,})(.*)", validators).unwrap();
}

