use synox::{StringProgram, blinkfill};

fn main() {
    let unpaired: &[Vec<&str>] = &[];
    let examples = &[(vec!["John Doe"], "J. Doe"),
                     (vec!["Alice Smith"], "A. Smith")];

    let prog = blinkfill::learn(unpaired, examples).unwrap();
    let result = prog.run(&["Bob Johnson"]).unwrap();
    println!("{}", result); // Должно вывести: "B. Johnson"
}

