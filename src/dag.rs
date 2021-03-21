use crate::graph;
use crate::input_data_graph::{Id, InputDataGraph};
use crate::language::{
    ColumnIndex, Direction, Occurrence, Position, StringExpression, StringIndex, StringProgram,
    SubstringExpression,
};
use crate::token::Token;
use std::collections::{HashMap, HashSet};

const EPSILON: usize = 1;
const KAPPA: usize = 15; // BlinkFill Section 7.3

type Node = usize;
type Edge = (Node, Node);

#[derive(Debug)]
pub struct Dag {
    start: Node,
    finish: Node,
    substrings: HashMap<Edge, Vec<SubstringExpressionSet>>,
}

impl Dag {
    fn nodes(&self) -> impl ExactSizeIterator<Item = &Node> + '_ {
        let unique_keys: HashSet<_> = self
            .substrings
            .keys()
            .flat_map(|edge| vec![&edge.0, &edge.1].into_iter())
            .collect();
        unique_keys.into_iter()
    }

    fn edges(&self) -> impl ExactSizeIterator<Item = &Edge> + '_ {
        self.substrings.keys()
    }

    fn new(input: &Vec<String>, output: &str, graph: &InputDataGraph, row: usize) -> Self {
        let mut substrings = HashMap::new();
        let n = output.len();

        for i in 0..n {
            for j in i + 1..n + 1 {
                let s = &output[i..j];
                // learn the constant string
                let mut exprs = vec![ConstantString(String::from(s))];
                // learn all substring expressions
                for (ci, input_str) in input.iter().enumerate() {
                    // find all instances of s (including overlapping ones) in input_str
                    let id = Id { row, col: ci };
                    let mut offset = 0;
                    while offset < input_str.len() {
                        match input_str[offset..].find(&s) {
                            None => {
                                break;
                            }
                            Some(start) => {
                                let l = offset + start;
                                let r = l + s.len();
                                let (l, r) = (StringIndex(l + 1), StringIndex(r + 1));
                                let substring_exprs =
                                    SubstringExpressionSet::generate_substring_set(
                                        id, l, r, &graph,
                                    );
                                exprs.push(substring_exprs);
                                offset = offset + start + 1;
                            }
                        }
                    }
                }
                substrings.insert((i, j), exprs);
            }
        }

        Self {
            start: 0,
            finish: n,
            substrings,
        }
    }

    fn intersection(&self, other: &Self) -> Self {
        // uses roughly the same renumbering technique as InputDataGraph::intersection()
        let mut renumber: HashMap<Edge, Node> = HashMap::new();
        let mut curr = 0;
        let mut number = |n1, n2| -> Node {
            *renumber.entry((n1, n2)).or_insert_with(|| {
                let v = curr;
                curr += 1;
                v
            })
        };

        let mut substrings = HashMap::new();
        for ((v1s, v1f), s1) in &self.substrings {
            for ((v2s, v2f), s2) in &other.substrings {
                let vs = number(*v1s, *v2s);
                let vf = number(*v1f, *v2f);
                let mut exprs = vec![];
                for e1 in s1 {
                    for e2 in s2 {
                        if let Some(e) = e1.intersection(&e2) {
                            exprs.push(e);
                        }
                    }
                }
                if !exprs.is_empty() {
                    substrings.insert((vs, vf), exprs);
                }
            }
        }

        Self {
            start: number(self.start, other.start),
            finish: number(self.finish, other.finish),
            substrings,
        }
    }

    fn learn_program_full(
        paired: &Vec<(Vec<String>, String)>,
        unpaired: &Vec<Vec<String>>,
    ) -> Option<impl StringProgram> {
        let all_unpaired: Vec<Vec<String>> = paired
            .iter()
            .map(|(i, _)| i)
            .chain(unpaired.iter())
            .cloned()
            .collect();
        let graph = InputDataGraph::new(&all_unpaired);
        Self::learn(paired, &graph).top_ranked_expression(&graph)
    }

    fn learn(paired: &Vec<(Vec<String>, String)>, graph: &InputDataGraph) -> Self {
        paired
            .iter()
            .enumerate()
            .map(|(row, (input, output))| Self::new(&input, output, &graph, row))
            .fold(None, |acc, x| -> Option<Self> {
                match acc {
                    Some(acc) => Some(acc.intersection(&x)),
                    None => Some(x),
                }
            })
            .unwrap()
    }

    fn top_ranked_expression(&self, graph: &InputDataGraph) -> Option<impl StringProgram> {
        let distances = graph.distances();
        let ranks = graph.rank_nodes(&distances);
        let mut best_by_edge: HashMap<Edge, (usize, SubstringExpression)> = HashMap::new();
        // compute distances for edges
        let idg_adj = graph::adjacency_map(graph.edges());
        let idg_inv = graph::invert_adjacency_map(&idg_adj);
        let rows = graph.num_rows();
        for (edge, expr_set_set) in &self.substrings {
            let mut best: Option<SubstringExpression> = None;
            let mut best_score = 0;
            for expr_set in expr_set_set {
                let expr;
                let score;
                match expr_set {
                    ConstantString(s) => {
                        expr = Some(SubstringExpression::ConstantString(s.clone()));
                        score = s.len() * s.len() * EPSILON;
                    }
                    SubstringSet(ci, p_l, p_r) => {
                        let key = |p: &'_ &PositionSet| -> usize {
                            match p {
                                ConstantPosition(_) => 0,
                                GraphNode(v) => ranks[&v],
                            }
                        };
                        let p_l = p_l.iter().max_by_key(key).unwrap();
                        let p_r = p_r.iter().max_by_key(key).unwrap();

                        // NOTE all of the programs captured in the dag are consistent with the
                        // input-output examples, but they aren't necessarily even valid for the
                        // rest of the inputs.
                        //
                        // XXX Maybe we should find all the consistent (p_l, p_r) pairs first
                        // instead of taking the max by key above, and then after that take the max
                        // by key, so we don't accidentally throw away a good example and then be
                        // left with one that doesn't work. The BlinkFill paper isn't very clear
                        // about avoiding generating invalid programs for the input strings that
                        // don't have output examples.

                        let sample = |p: &PositionSet| -> Position {
                            match p {
                                ConstantPosition(k) => Position::ConstantPosition(*k),
                                GraphNode(v) => {
                                    // check in-edges
                                    if let Some(vss) = idg_inv.get(v) {
                                        for vs in vss {
                                            if let Some(toks) = graph.tokens.get(&(*vs, *v)) {
                                                for (tok, occ) in toks {
                                                    return Position::Match(
                                                        tok.clone(),
                                                        *occ,
                                                        Direction::End,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    // check out-edges
                                    if let Some(vfs) = idg_adj.get(v) {
                                        for vf in vfs {
                                            if let Some(toks) = graph.tokens.get(&(*v, *vf)) {
                                                for (tok, occ) in toks {
                                                    return Position::Match(
                                                        tok.clone(),
                                                        *occ,
                                                        Direction::Start,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    // we should never get here; if we did, it means our
                                    // PositionSet was invalid
                                    panic!("top_ranked_expression: no tokens for graph node");
                                }
                            }
                        };

                        let len: usize;
                        let mut bad = false;
                        match (p_l, p_r) {
                            (ConstantPosition(k1), ConstantPosition(k2)) => {
                                len = k2.0 as usize - k1.0 as usize;
                            }
                            (ConstantPosition(k), GraphNode(v)) => {
                                let k = k.0 as usize;
                                let mut sum = 0;
                                for (_, si) in &graph.labels[v] {
                                    if si.0 > k {
                                        sum += si.0 - k;
                                    } else {
                                        bad = true;
                                        break;
                                    }
                                }
                                len = sum / rows;
                            }
                            (GraphNode(v), ConstantPosition(k)) => {
                                // similar to the above case
                                // note: difference direction is opposite the above case
                                let k = k.0 as usize;
                                let mut sum = 0;
                                for (_, si) in &graph.labels[v] {
                                    if k > si.0 {
                                        sum += k - si.0;
                                    } else {
                                        bad = true;
                                        break;
                                    }
                                }
                                len = sum / rows;
                            }
                            (GraphNode(v1), GraphNode(v2)) => {
                                let mut sum = 0;
                                for (id, si1) in &graph.labels[v1] {
                                    let si2 = graph.labels[v2][id];
                                    if si2.0 > si1.0 {
                                        sum += si2.0 - si1.0;
                                    } else {
                                        bad = true;
                                        break;
                                    }
                                }
                                len = sum / rows;
                            }
                        }
                        expr = if !bad {
                            Some(SubstringExpression::Substring(
                                *ci,
                                sample(p_l),
                                sample(p_r),
                            ))
                        } else {
                            None
                        };
                        score = len * len * KAPPA;
                    }
                }
                if let Some(expr) = expr {
                    if best == None || score > best_score {
                        best = Some(expr);
                        best_score = score;
                    }
                }
            }
            if let Some(best) = best {
                best_by_edge.insert(*edge, (best_score, best));
            }
        }
        // find shortest path
        let adj = graph::adjacency_map(best_by_edge.keys());
        let path = graph::shortest_path_dag(&self.start, &self.finish, &adj, |v1, v2| {
            // negating because graph finds lowest cost path, we want highest score
            -(best_by_edge[&(*v1, *v2)].0 as isize)
        })?;
        Some(StringExpression(
            path.iter()
                .map(|e| best_by_edge.remove(e).unwrap().1)
                .collect(),
        ))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SubstringExpressionSet {
    ConstantString(String),
    SubstringSet(ColumnIndex, HashSet<PositionSet>, HashSet<PositionSet>),
}

use SubstringExpressionSet::*;

impl SubstringExpressionSet {
    // returns a SubstringSet
    fn generate_substring_set(
        id: Id,
        l: StringIndex,
        r: StringIndex,
        graph: &InputDataGraph,
    ) -> Self {
        let mut v_l = HashSet::new();
        let mut v_r = HashSet::new();
        for (v, labels) in &graph.labels {
            if labels.get(&id) == Some(&l) {
                v_l.insert(GraphNode(*v));
            } else if labels.get(&id) == Some(&r) {
                v_r.insert(GraphNode(*v));
            }
        }
        v_l.insert(ConstantPosition(Occurrence(l.0 as isize)));
        v_r.insert(ConstantPosition(Occurrence(r.0 as isize)));
        SubstringSet(ColumnIndex(id.col), v_l, v_r)
    }

    fn denote(&self, graph: &InputDataGraph) -> HashSet<SubstringExpression> {
        let mut set: HashSet<SubstringExpression> = HashSet::new();
        match self {
            ConstantString(s) => {
                set.insert(SubstringExpression::ConstantString(s.clone()));
            }
            SubstringSet(ci, p_l, p_r) => {
                for p_l in p_l.iter().flat_map(|p_l| p_l.denote(graph)) {
                    for p_r in p_r.iter().flat_map(|p_r| p_r.denote(graph)) {
                        set.insert(SubstringExpression::Substring(
                            *ci,
                            p_l.clone(),
                            p_r.clone(),
                        ));
                    }
                }
            }
        }
        set
    }

    fn intersection(&self, other: &Self) -> Option<Self> {
        match (self, other) {
            (ConstantString(s1), ConstantString(s2)) if s1 == s2 => {
                Some(ConstantString(s1.clone()))
            }
            (SubstringSet(c1, p1_l, p1_r), SubstringSet(c2, p2_l, p2_r)) if c1 == c2 => {
                // return None if either intersection is empty; this is not necessary for
                // correctness but it's a performance optimization
                let p_l: HashSet<_> = p1_l.intersection(p2_l).cloned().collect();
                if p_l.is_empty() {
                    return None;
                }
                let p_r: HashSet<_> = p1_r.intersection(p2_r).cloned().collect();
                if p_r.is_empty() {
                    return None;
                }
                Some(SubstringSet(c1.clone(), p_l, p_r))
            }
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
enum PositionSet {
    ConstantPosition(Occurrence),
    GraphNode(Node),
}

use PositionSet::*;

impl PositionSet {
    fn denote(&self, graph: &InputDataGraph) -> HashSet<Position> {
        let mut set: HashSet<Position> = HashSet::new();
        match self {
            ConstantPosition(k) => {
                set.insert(Position::ConstantPosition(*k));
            }
            GraphNode(v) => {
                // find all edges that end at v or start at v
                for ((vs, vf), tok_occs) in &graph.tokens {
                    if vs == v {
                        for (tok, occ) in tok_occs {
                            set.insert(Position::Match(tok.clone(), *occ, Direction::Start));
                        }
                    } else if vf == v {
                        for (tok, occ) in tok_occs {
                            set.insert(Position::Match(tok.clone(), *occ, Direction::End));
                        }
                    }
                }
            }
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_substring_set() {
        // generate the graph from BlinkFill Fig. 14
        let strs = vec![
            vec![String::from("Mumbai, India")],
            vec![String::from("Los Angeles, United States of America")],
            vec![String::from("Newark, United States")],
            vec![String::from("New York, United States of America")],
            vec![String::from("Wellington, New Zealand")],
            vec![String::from("New Delhi, India")],
        ];
        let graph = InputDataGraph::new(&strs);
        // find the substring expression set that generates "India" from the 1st string
        let sub = SubstringExpressionSet::generate_substring_set(
            Id::new(0, 0),
            StringIndex(9),
            StringIndex(14),
            &graph,
        );
        let sub_denote = sub.denote(&graph);

        // spot check a couple things
        assert!(sub_denote.contains(&SubstringExpression::Substring(
            ColumnIndex(0),
            Position::Match(Token::ProperCase, Occurrence(2), Direction::Start),
            Position::Match(Token::ProperCase, Occurrence(2), Direction::End)
        )));
        assert!(sub_denote.contains(&SubstringExpression::Substring(
            ColumnIndex(0),
            Position::Match(Token::Caps, Occurrence(2), Direction::Start),
            Position::Match(Token::ProperCase, Occurrence(2), Direction::End)
        )));
        assert!(sub_denote.contains(&SubstringExpression::Substring(
            ColumnIndex(0),
            Position::Match(Token::Alphabets, Occurrence(-1), Direction::Start),
            Position::Match(Token::End, Occurrence(1), Direction::Start)
        )));
        // even though this next pattern would occur if the graph were built from only the first
        // string, this pattern doesn't match in the other strings, so it does not appear
        assert!(!sub_denote.contains(&SubstringExpression::Substring(
            ColumnIndex(0),
            Position::Match(
                Token::Literal(String::from("Mumbai, ")),
                Occurrence(1),
                Direction::End
            ),
            Position::Match(Token::End, Occurrence(1), Direction::Start)
        )));
        assert!(sub_denote.contains(&SubstringExpression::Substring(
            ColumnIndex(0),
            Position::Match(
                Token::Literal(String::from(", ")),
                Occurrence(1),
                Direction::End
            ),
            Position::Match(Token::End, Occurrence(1), Direction::Start)
        )));

        // make sure all the string programs generate the right string
        for prog in sub_denote {
            let output = prog.run(&vec![String::from("Mumbai, India")]);
            assert_eq!(output.unwrap(), "India");
        }
    }

    #[test]
    fn generate_substring_set_single() {
        // similar to the negated case from above, with a different graph, should appear
        let strs = vec![
            vec![String::from("Shrewsbury, MA")],
            vec![String::from("Shrewsbury, United Kingdom")],
        ];
        let graph = InputDataGraph::new(&strs);
        // find the substring expression set that generates "MA" from the 1st string
        let sub = SubstringExpressionSet::generate_substring_set(
            Id::new(0, 0),
            StringIndex(13),
            StringIndex(15),
            &graph,
        );
        let sub_denote = sub.denote(&graph);
        assert!(sub_denote.contains(&SubstringExpression::Substring(
            ColumnIndex(0),
            Position::Match(
                Token::Literal(String::from("Shrewsbury, ")),
                Occurrence(1),
                Direction::End
            ),
            Position::Match(Token::End, Occurrence(1), Direction::Start)
        )));
    }

    fn all_for(
        dag: &Dag,
        graph: &InputDataGraph,
        n1: Node,
        n2: Node,
    ) -> HashSet<SubstringExpression> {
        dag.substrings
            .get(&(n1, n2))
            .unwrap()
            .iter()
            .flat_map(|e| e.denote(&graph))
            .collect()
    }

    #[test]
    fn generate_dag() {
        let strs = vec![
            vec![String::from("Mumbai, India")],
            vec![String::from("Los Angeles, United States of America")],
            vec![String::from("Newark, United States")],
            vec![String::from("New York, United States of America")],
            vec![String::from("Wellington, New Zealand")],
            vec![String::from("New Delhi, India")],
        ];
        let graph = InputDataGraph::new(&strs);
        let dag = Dag::new(&strs[0], "India", &graph, 0);
        assert_eq!(dag.nodes().len(), 6);
        // some spot checks
        assert!(all_for(&dag, &graph, 0, 3)
            .contains(&SubstringExpression::ConstantString(String::from("Ind"))));
        assert!(
            all_for(&dag, &graph, 0, 1).contains(&SubstringExpression::Substring(
                ColumnIndex(0),
                Position::Match(Token::Caps, Occurrence(2), Direction::Start),
                Position::ConstantPosition(Occurrence(10))
            ))
        );
        assert!(
            all_for(&dag, &graph, 0, 5).contains(&SubstringExpression::Substring(
                ColumnIndex(0),
                Position::Match(Token::ProperCase, Occurrence(-1), Direction::Start),
                Position::Match(Token::End, Occurrence(1), Direction::Start),
            ))
        );
    }

    #[test]
    fn learn() {
        let strs = vec![
            vec![String::from("Mumbai, India")],
            vec![String::from("Los Angeles, United States of America")],
            vec![String::from("Newark, United States")],
            vec![String::from("New York, United States of America")],
            vec![String::from("Wellington, New Zealand")],
            vec![String::from("New Delhi, India")],
        ];
        let graph = InputDataGraph::new(&strs);
        let examples = vec![
            (strs[0].clone(), String::from("India")),
            (strs[1].clone(), String::from("United States of America")),
        ];
        let dag = Dag::learn(&examples, &graph);
        // check all expressions that extract output in one go
        let exprs = all_for(&dag, &graph, dag.start, dag.finish);
        for e in &exprs {
            for ex in &examples {
                assert_eq!(e.run(&ex.0).unwrap(), ex.1);
            }
        }
        // spot-check
        assert!(!exprs.contains(&SubstringExpression::Substring(
            ColumnIndex(0),
            Position::Match(Token::ProperCase, Occurrence(-1), Direction::Start),
            Position::Match(Token::End, Occurrence(1), Direction::Start),
        )));
        assert!(exprs.contains(&SubstringExpression::Substring(
            ColumnIndex(0),
            Position::Match(
                Token::Literal(String::from(", ")),
                Occurrence(1),
                Direction::End
            ),
            Position::Match(Token::End, Occurrence(1), Direction::Start),
        )));
        // check final program
        let best = dag.top_ranked_expression(&graph).unwrap();
        let expected = vec![
            "United States",
            "United States of America",
            "New Zealand",
            "India",
        ];
        for (i, s) in strs[2..].iter().enumerate() {
            assert_eq!(best.run(s).unwrap(), expected[i]);
        }
    }

    #[test]
    fn learn_2() {
        let strs = vec![
            vec![String::from("323-708-7700")],
            vec![String::from("(425).706.7709")],
            vec![String::from("510.220.5586")],
            vec![String::from("(471)-378-3829")],
        ];
        let graph = InputDataGraph::new(&strs);
        let examples = vec![
            (strs[0].clone(), String::from("323-708-7700")),
            (strs[1].clone(), String::from("425-706-7709")),
        ];
        let dag = Dag::learn(&examples, &graph);
        let best = dag.top_ranked_expression(&graph).unwrap();
        let expected = vec!["510-220-5586", "471-378-3829"];
        for (i, s) in strs[2..].iter().enumerate() {
            assert_eq!(best.run(s).unwrap(), expected[i]);
        }
    }

    #[test]
    fn learn_3() {
        let strs = vec![
            vec![String::from("Brandon Henry Saunders")],
            vec![String::from("Dafna Q. Chen")],
            vec![String::from("William Lee")],
            vec![String::from("Danelle D. Saunders")],
            vec![String::from("Emilio William Conception")],
        ];
        let graph = InputDataGraph::new(&strs);
        let examples = vec![
            (strs[0].clone(), String::from("B.S.")),
            (strs[1].clone(), String::from("D.C.")),
        ];
        let dag = Dag::learn(&examples, &graph);
        let best = dag.top_ranked_expression(&graph).unwrap();
        let expected = vec!["W.L.", "D.S.", "E.C."];
        for (i, s) in strs[2..].iter().enumerate() {
            assert_eq!(best.run(s).unwrap(), expected[i]);
        }
    }

    #[test]
    fn learn_4() {
        let strs = vec![
            vec![String::from("GOPR0365.MP4.mp4")],
            vec![String::from("GOPR0411.MP4.mp4")],
            vec![String::from("GOPR0329.MP4.mp4")],
        ];
        let graph = InputDataGraph::new(&strs);
        let examples = vec![(strs[0].clone(), String::from("GOPR0365.mp4"))];
        let dag = Dag::learn(&examples, &graph);
        let best = dag.top_ranked_expression(&graph).unwrap();
        let expected = vec!["GOPR0411.mp4", "GOPR0329.mp4"];
        for (i, s) in strs[1..].iter().enumerate() {
            assert_eq!(best.run(s).unwrap(), expected[i]);
        }
    }

    #[test]
    fn learn_5() {
        let strs = vec![
            vec![String::from("IMG_3246.JPG")],
            vec![String::from("GOPR0411.MP4")],
            vec![String::from("DSC_0324.jpg")],
            vec![String::from("DSC0324.jpg")],
            vec![String::from("RD392.HEIC")],
        ];
        let graph = InputDataGraph::new(&strs);
        let examples = vec![
            (strs[0].clone(), String::from("IMG_3246")),
            (strs[1].clone(), String::from("GOPR0411")),
        ];
        let dag = Dag::learn(&examples, &graph);
        let best = dag.top_ranked_expression(&graph).unwrap();
        let expected = vec!["DSC_0324", "DSC0324", "RD392"];
        for (i, s) in strs[2..].iter().enumerate() {
            assert_eq!(best.run(s).unwrap(), expected[i]);
        }
    }
}
