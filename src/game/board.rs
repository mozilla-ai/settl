use rand::seq::SliceRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Axial coordinate for a hex tile (pointy-top orientation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HexCoord {
    pub q: i8,
    pub r: i8,
}

impl HexCoord {
    pub const fn new(q: i8, r: i8) -> Self {
        Self { q, r }
    }

    /// The six neighbours of this hex in axial coordinates.
    pub fn neighbors(self) -> [HexCoord; 6] {
        [
            HexCoord::new(self.q + 1, self.r - 1), // NE
            HexCoord::new(self.q + 1, self.r),     // E
            HexCoord::new(self.q, self.r + 1),     // SE
            HexCoord::new(self.q - 1, self.r + 1), // SW
            HexCoord::new(self.q - 1, self.r),     // W
            HexCoord::new(self.q, self.r - 1),     // NW
        ]
    }

    /// The six vertices surrounding this hex in canonical form.
    pub fn vertices(&self) -> [VertexCoord; 6] {
        hex_vertices(*self)
    }

    /// The six edges of this hex in canonical form.
    pub fn edges(&self) -> [EdgeCoord; 6] {
        hex_edges(*self)
    }
}

// ---------------------------------------------------------------------------
// Vertex
// ---------------------------------------------------------------------------

/// Whether a vertex sits at the top (North) or bottom (South) of a hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VertexDirection {
    North,
    South,
}

/// A vertex on the hex grid, identified by a hex coordinate and direction.
///
/// In pointy-top axial coordinates every `(hex, dir)` pair maps to a unique
/// geometric point — no two distinct pairs refer to the same vertex.
///
/// The six vertices of hex (q, r), clockwise from top:
/// ```text
///      v0 = North(q, r)
///      v1 = South(q+1, r-1)      (NE)
///      v2 = North(q, r+1)        (SE)
///      v3 = South(q, r)
///      v4 = North(q-1, r+1)      (SW)
///      v5 = South(q, r-1)        (NW)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VertexCoord {
    pub hex: HexCoord,
    pub dir: VertexDirection,
}

impl VertexCoord {
    pub const fn new(hex: HexCoord, dir: VertexDirection) -> Self {
        Self { hex, dir }
    }
}

// ---------------------------------------------------------------------------
// Edge
// ---------------------------------------------------------------------------

/// Canonical edge direction.  Only NorthEast, East and SouthEast are stored;
/// the opposite three (SW, W, NW) are expressed as NE/E/SE of the
/// neighbouring hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeDirection {
    NorthEast,
    East,
    SouthEast,
}

/// An edge on the hex grid, identified by a hex coordinate and direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EdgeCoord {
    pub hex: HexCoord,
    pub dir: EdgeDirection,
}

impl EdgeCoord {
    pub const fn new(hex: HexCoord, dir: EdgeDirection) -> Self {
        Self { hex, dir }
    }
}

impl std::fmt::Display for EdgeCoord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dir_name = match self.dir {
            EdgeDirection::NorthEast => "NE",
            EdgeDirection::East => "E",
            EdgeDirection::SouthEast => "SE",
        };
        write!(f, "({},{},{})", self.hex.q, self.hex.r, dir_name)
    }
}

// ---------------------------------------------------------------------------
// Resource / Terrain
// ---------------------------------------------------------------------------

/// The five tradeable resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Resource {
    Wood,
    Brick,
    Sheep,
    Wheat,
    Ore,
}

impl Resource {
    /// Returns all five resource variants.
    pub fn all() -> &'static [Resource; 5] {
        &[
            Resource::Wood,
            Resource::Brick,
            Resource::Sheep,
            Resource::Wheat,
            Resource::Ore,
        ]
    }
}

impl std::fmt::Display for Resource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Resource::Wood => write!(f, "Wood"),
            Resource::Brick => write!(f, "Brick"),
            Resource::Sheep => write!(f, "Sheep"),
            Resource::Wheat => write!(f, "Wheat"),
            Resource::Ore => write!(f, "Ore"),
        }
    }
}

/// Terrain type for each hex tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Terrain {
    Forest,    // produces Wood
    Hills,     // produces Brick
    Pasture,   // produces Sheep
    Fields,    // produces Wheat
    Mountains, // produces Ore
    Desert,
}

impl Terrain {
    /// Returns the resource produced by this terrain, or `None` for Desert.
    pub fn resource(self) -> Option<Resource> {
        match self {
            Terrain::Forest => Some(Resource::Wood),
            Terrain::Hills => Some(Resource::Brick),
            Terrain::Pasture => Some(Resource::Sheep),
            Terrain::Fields => Some(Resource::Wheat),
            Terrain::Mountains => Some(Resource::Ore),
            Terrain::Desert => None,
        }
    }

    /// Two-character abbreviation for display in ASCII and TUI boards.
    pub fn abbr(self) -> &'static str {
        match self {
            Terrain::Forest => "Wo",
            Terrain::Hills => "Bk",
            Terrain::Pasture => "Sh",
            Terrain::Fields => "Wh",
            Terrain::Mountains => "Or",
            Terrain::Desert => "De",
        }
    }
}

// ---------------------------------------------------------------------------
// Hex tile
// ---------------------------------------------------------------------------

/// A single hex tile on the board.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hex {
    pub coord: HexCoord,
    pub terrain: Terrain,
    /// 2-12 (except 7); `None` for the desert.
    pub number_token: Option<u8>,
}

// ---------------------------------------------------------------------------
// Port
// ---------------------------------------------------------------------------

/// Type of harbour/port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortType {
    /// Trade any 3 identical resources for 1 of any other.
    Generic, // 3:1
             // Specialised 2:1 ports are deferred.
}

/// A harbour on the coast, accessible from two vertices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Port {
    pub port_type: PortType,
    /// The two coastal vertices that give access to this port.
    pub vertices: (VertexCoord, VertexCoord),
}

// ---------------------------------------------------------------------------
// Board
// ---------------------------------------------------------------------------

/// The Catan board: hex tiles, ports and their layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Board {
    pub hexes: Vec<Hex>,
    pub ports: Vec<Port>,
}

// ---------------------------------------------------------------------------
// Board coordinate helpers
// ---------------------------------------------------------------------------

/// All 19 hex coordinates for a standard Catan board (3-4-5-4-3 layout).
pub fn board_hex_coords() -> Vec<HexCoord> {
    vec![
        // Row 0 (r = -2): 3 hexes
        HexCoord::new(0, -2),
        HexCoord::new(1, -2),
        HexCoord::new(2, -2),
        // Row 1 (r = -1): 4 hexes
        HexCoord::new(-1, -1),
        HexCoord::new(0, -1),
        HexCoord::new(1, -1),
        HexCoord::new(2, -1),
        // Row 2 (r = 0): 5 hexes
        HexCoord::new(-2, 0),
        HexCoord::new(-1, 0),
        HexCoord::new(0, 0),
        HexCoord::new(1, 0),
        HexCoord::new(2, 0),
        // Row 3 (r = 1): 4 hexes
        HexCoord::new(-2, 1),
        HexCoord::new(-1, 1),
        HexCoord::new(0, 1),
        HexCoord::new(1, 1),
        // Row 4 (r = 2): 3 hexes
        HexCoord::new(-2, 2),
        HexCoord::new(-1, 2),
        HexCoord::new(0, 2),
    ]
}

/// Returns `true` if the given coordinate is one of the 19 board hexes.
#[allow(dead_code)]
pub fn is_board_hex(c: HexCoord) -> bool {
    match c.r {
        -2 => (0..=2).contains(&c.q),
        -1 => (-1..=2).contains(&c.q),
        0 => (-2..=2).contains(&c.q),
        1 => (-2..=1).contains(&c.q),
        2 => (-2..=0).contains(&c.q),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Canonical normalization
// ---------------------------------------------------------------------------

/// Return the canonical form of a vertex coordinate.
///
/// In pointy-top axial coordinates, each `(hex, dir)` pair maps to a unique
/// geometric point, so the canonical form is simply the identity.
#[allow(dead_code)]
pub fn canonical_vertex(v: VertexCoord) -> VertexCoord {
    v
}

/// Return the canonical form of an edge coordinate.
///
/// Since [`EdgeDirection`] only has NE, E and SE variants, the representation
/// is already canonical.
#[allow(dead_code)]
pub fn canonical_edge(e: EdgeCoord) -> EdgeCoord {
    e
}

// ---------------------------------------------------------------------------
// Vertex ↔ Hex adjacency
// ---------------------------------------------------------------------------

/// Returns the 3 hexes that touch a given vertex.
///
/// Not all returned hexes need be on the board; the caller should filter
/// with [`is_board_hex`] when needed.
///
///   North(q, r) is touched by: (q, r), (q, r-1), (q+1, r-1)
///   South(q, r) is touched by: (q, r), (q, r+1), (q-1, r+1)
pub fn vertex_neighbors(v: VertexCoord) -> Vec<HexCoord> {
    let q = v.hex.q;
    let r = v.hex.r;
    match v.dir {
        VertexDirection::North => vec![
            HexCoord::new(q, r),
            HexCoord::new(q, r - 1),
            HexCoord::new(q + 1, r - 1),
        ],
        VertexDirection::South => vec![
            HexCoord::new(q, r),
            HexCoord::new(q, r + 1),
            HexCoord::new(q - 1, r + 1),
        ],
    }
}

/// Returns the two hexes on either side of an edge.
///
///   NE(q, r): between (q, r) and (q+1, r-1)
///   E(q, r):  between (q, r) and (q+1, r)
///   SE(q, r): between (q, r) and (q, r+1)
pub fn edge_neighbors(e: EdgeCoord) -> Vec<HexCoord> {
    let q = e.hex.q;
    let r = e.hex.r;
    match e.dir {
        EdgeDirection::NorthEast => vec![HexCoord::new(q, r), HexCoord::new(q + 1, r - 1)],
        EdgeDirection::East => vec![HexCoord::new(q, r), HexCoord::new(q + 1, r)],
        EdgeDirection::SouthEast => vec![HexCoord::new(q, r), HexCoord::new(q, r + 1)],
    }
}

// ---------------------------------------------------------------------------
// Vertex ↔ Vertex adjacency
// ---------------------------------------------------------------------------

/// Returns the 3 vertices connected to `v` by an edge.
///
///   Neighbours of North(q, r):
///     South(q, r-1)       via SE(q, r-1)
///     South(q+1, r-1)     via NE(q, r)
///     South(q+1, r-2)     via E(q, r-1)
///
///   Neighbours of South(q, r):
///     North(q, r+1)       via SE(q, r)
///     North(q-1, r+1)     via NE(q-1, r+1)
///     North(q-1, r+2)     via E(q-1, r+1)
pub fn adjacent_vertices(v: VertexCoord) -> Vec<VertexCoord> {
    let q = v.hex.q;
    let r = v.hex.r;
    match v.dir {
        VertexDirection::North => vec![
            VertexCoord::new(HexCoord::new(q + 1, r - 1), VertexDirection::South),
            VertexCoord::new(HexCoord::new(q, r - 1), VertexDirection::South),
            VertexCoord::new(HexCoord::new(q + 1, r - 2), VertexDirection::South),
        ],
        VertexDirection::South => vec![
            VertexCoord::new(HexCoord::new(q, r + 1), VertexDirection::North),
            VertexCoord::new(HexCoord::new(q - 1, r + 1), VertexDirection::North),
            VertexCoord::new(HexCoord::new(q - 1, r + 2), VertexDirection::North),
        ],
    }
}

/// Returns the 3 edges that touch a given vertex.
///
///   Edges at North(q, r):
///     NE(q, r)
///     SE(q, r-1)
///     E(q, r-1)
///
///   Edges at South(q, r):
///     SE(q, r)
///     NE(q-1, r+1)
///     E(q-1, r+1)
pub fn adjacent_edges(v: VertexCoord) -> Vec<EdgeCoord> {
    let q = v.hex.q;
    let r = v.hex.r;
    match v.dir {
        VertexDirection::North => vec![
            EdgeCoord::new(HexCoord::new(q, r), EdgeDirection::NorthEast),
            EdgeCoord::new(HexCoord::new(q, r - 1), EdgeDirection::SouthEast),
            EdgeCoord::new(HexCoord::new(q, r - 1), EdgeDirection::East),
        ],
        VertexDirection::South => vec![
            EdgeCoord::new(HexCoord::new(q, r), EdgeDirection::SouthEast),
            EdgeCoord::new(HexCoord::new(q - 1, r + 1), EdgeDirection::NorthEast),
            EdgeCoord::new(HexCoord::new(q - 1, r + 1), EdgeDirection::East),
        ],
    }
}

// ---------------------------------------------------------------------------
// Edge ↔ Vertex
// ---------------------------------------------------------------------------

/// Returns the two vertices at the endpoints of an edge.
///
///   NE(q, r): North(q, r) — South(q+1, r-1)
///   E(q, r):  South(q+1, r-1) — North(q, r+1)
///   SE(q, r): North(q, r+1) — South(q, r)
pub fn edge_vertices(e: EdgeCoord) -> (VertexCoord, VertexCoord) {
    let q = e.hex.q;
    let r = e.hex.r;
    match e.dir {
        EdgeDirection::NorthEast => (
            VertexCoord::new(HexCoord::new(q, r), VertexDirection::North),
            VertexCoord::new(HexCoord::new(q + 1, r - 1), VertexDirection::South),
        ),
        EdgeDirection::East => (
            VertexCoord::new(HexCoord::new(q + 1, r - 1), VertexDirection::South),
            VertexCoord::new(HexCoord::new(q, r + 1), VertexDirection::North),
        ),
        EdgeDirection::SouthEast => (
            VertexCoord::new(HexCoord::new(q, r + 1), VertexDirection::North),
            VertexCoord::new(HexCoord::new(q, r), VertexDirection::South),
        ),
    }
}

// ---------------------------------------------------------------------------
// Hex → Vertices / Edges
// ---------------------------------------------------------------------------

/// Returns all 6 vertices of a hex in canonical form (clockwise from top).
///
///   v0 = North(q, r)
///   v1 = South(q+1, r-1)
///   v2 = North(q, r+1)
///   v3 = South(q, r)
///   v4 = North(q-1, r+1)
///   v5 = South(q, r-1)
pub fn hex_vertices(h: HexCoord) -> [VertexCoord; 6] {
    let q = h.q;
    let r = h.r;
    [
        VertexCoord::new(HexCoord::new(q, r), VertexDirection::North), // v0
        VertexCoord::new(HexCoord::new(q + 1, r - 1), VertexDirection::South), // v1
        VertexCoord::new(HexCoord::new(q, r + 1), VertexDirection::North), // v2
        VertexCoord::new(HexCoord::new(q, r), VertexDirection::South), // v3
        VertexCoord::new(HexCoord::new(q - 1, r + 1), VertexDirection::North), // v4
        VertexCoord::new(HexCoord::new(q, r - 1), VertexDirection::South), // v5
    ]
}

/// Returns all 6 edges of a hex in canonical form (clockwise from NE).
///
///   edge 0 = NE(q, r)
///   edge 1 = E(q, r)
///   edge 2 = SE(q, r)
///   edge 3 = SW(q, r) = NE(q-1, r+1)
///   edge 4 = W(q, r)  = E(q-1, r)
///   edge 5 = NW(q, r) = SE(q, r-1)
pub fn hex_edges(h: HexCoord) -> [EdgeCoord; 6] {
    let q = h.q;
    let r = h.r;
    [
        EdgeCoord::new(HexCoord::new(q, r), EdgeDirection::NorthEast),
        EdgeCoord::new(HexCoord::new(q, r), EdgeDirection::East),
        EdgeCoord::new(HexCoord::new(q, r), EdgeDirection::SouthEast),
        EdgeCoord::new(HexCoord::new(q - 1, r + 1), EdgeDirection::NorthEast), // SW
        EdgeCoord::new(HexCoord::new(q - 1, r), EdgeDirection::East),          // W
        EdgeCoord::new(HexCoord::new(q, r - 1), EdgeDirection::SouthEast),     // NW
    ]
}

// ---------------------------------------------------------------------------
// Board generation
// ---------------------------------------------------------------------------

/// Standard Catan terrain distribution (19 tiles).
fn standard_terrains() -> Vec<Terrain> {
    vec![
        Terrain::Forest,
        Terrain::Forest,
        Terrain::Forest,
        Terrain::Forest,
        Terrain::Pasture,
        Terrain::Pasture,
        Terrain::Pasture,
        Terrain::Pasture,
        Terrain::Fields,
        Terrain::Fields,
        Terrain::Fields,
        Terrain::Fields,
        Terrain::Hills,
        Terrain::Hills,
        Terrain::Hills,
        Terrain::Mountains,
        Terrain::Mountains,
        Terrain::Mountains,
        Terrain::Desert,
    ]
}

/// Standard Catan number tokens (18 tokens for 18 non-desert hexes).
fn standard_number_tokens() -> Vec<u8> {
    vec![2, 3, 3, 4, 4, 5, 5, 6, 6, 8, 8, 9, 9, 10, 10, 11, 11, 12]
}

/// Check whether any two adjacent hexes both carry a 6 or 8 token.
fn has_adjacent_red_numbers(hexes: &[Hex]) -> bool {
    use std::collections::HashMap;
    let token_map: HashMap<HexCoord, Option<u8>> =
        hexes.iter().map(|h| (h.coord, h.number_token)).collect();

    for hex in hexes {
        if let Some(n) = hex.number_token {
            if n == 6 || n == 8 {
                for nb in hex.coord.neighbors() {
                    if let Some(Some(nb_n)) = token_map.get(&nb) {
                        if *nb_n == 6 || *nb_n == 8 {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Fixed coastal port positions for the standard Catan board.
///
/// Each port is placed along a coastal edge, giving two coastal vertices.
/// All 9 ports are Generic (3:1) for now; specialised 2:1 ports are deferred.
fn standard_ports() -> Vec<Port> {
    let port_vertex_pairs: [(VertexCoord, VertexCoord); 9] = [
        // Top edge, between (0,-2) and (1,-2)
        (
            VertexCoord::new(HexCoord::new(0, -2), VertexDirection::North),
            VertexCoord::new(HexCoord::new(1, -3), VertexDirection::South),
        ),
        // Top-right, above (2,-2)
        (
            VertexCoord::new(HexCoord::new(2, -2), VertexDirection::North),
            VertexCoord::new(HexCoord::new(3, -3), VertexDirection::South),
        ),
        // Right upper, E edge of (2,-1)
        (
            VertexCoord::new(HexCoord::new(3, -2), VertexDirection::South),
            VertexCoord::new(HexCoord::new(3, -1), VertexDirection::North),
        ),
        // Right lower, E edge of (2,0)
        (
            VertexCoord::new(HexCoord::new(3, -1), VertexDirection::South),
            VertexCoord::new(HexCoord::new(3, 0), VertexDirection::North),
        ),
        // Bottom-right, SE edge of (1,1)
        (
            VertexCoord::new(HexCoord::new(1, 2), VertexDirection::North),
            VertexCoord::new(HexCoord::new(1, 1), VertexDirection::South),
        ),
        // Bottom, S of (0,2)
        (
            VertexCoord::new(HexCoord::new(0, 2), VertexDirection::South),
            VertexCoord::new(HexCoord::new(0, 3), VertexDirection::North),
        ),
        // Bottom-left, SW edge of (-2,2)
        (
            VertexCoord::new(HexCoord::new(-2, 3), VertexDirection::North),
            VertexCoord::new(HexCoord::new(-2, 2), VertexDirection::South),
        ),
        // Left lower, W edge of (-2,1)
        (
            VertexCoord::new(HexCoord::new(-3, 2), VertexDirection::North),
            VertexCoord::new(HexCoord::new(-3, 1), VertexDirection::South),
        ),
        // Left upper, NW edge of (-1,-1)
        (
            VertexCoord::new(HexCoord::new(-1, -1), VertexDirection::North),
            VertexCoord::new(HexCoord::new(-1, -2), VertexDirection::South),
        ),
    ];

    port_vertex_pairs
        .into_iter()
        .map(|(v1, v2)| Port {
            port_type: PortType::Generic,
            vertices: (v1, v2),
        })
        .collect()
}

impl Board {
    /// The 19 hex positions for a standard 3-4-5-4-3 board in axial coords.
    #[allow(dead_code)]
    pub fn hex_positions() -> Vec<HexCoord> {
        board_hex_coords()
    }

    /// Whether a hex coordinate exists on this board.
    pub fn has_hex(&self, coord: HexCoord) -> bool {
        self.hexes.iter().any(|h| h.coord == coord)
    }

    /// Returns the set of all valid vertex coordinates on this board.
    #[allow(dead_code)]
    pub fn all_vertices(&self) -> Vec<VertexCoord> {
        let mut verts: Vec<VertexCoord> = Vec::new();
        for hex in &self.hexes {
            for v in hex.coord.vertices() {
                if !verts.contains(&v) {
                    verts.push(v);
                }
            }
        }
        verts
    }

    /// Returns the set of all valid edge coordinates on this board.
    #[allow(dead_code)]
    pub fn all_edges(&self) -> Vec<EdgeCoord> {
        let mut edges: Vec<EdgeCoord> = Vec::new();
        for hex in &self.hexes {
            for e in hex.coord.edges() {
                if !edges.contains(&e) {
                    edges.push(e);
                }
            }
        }
        edges
    }

    /// Find the desert hex coordinate.
    pub fn desert_hex(&self) -> Option<HexCoord> {
        self.hexes
            .iter()
            .find(|h| h.terrain == Terrain::Desert)
            .map(|h| h.coord)
    }

    /// Create a deterministic standard Catan board (unshuffled).
    ///
    /// Useful for tests and demos.
    #[allow(dead_code)]
    pub fn default_board() -> Self {
        use Terrain::*;

        let coords = board_hex_coords();

        // Standard terrain distribution placed in coordinate order.
        let terrains = vec![
            Hills, Hills, Hills, Forest, Forest, Forest, Forest, Mountains, Mountains, Mountains,
            Fields, Fields, Fields, Fields, Pasture, Pasture, Pasture, Pasture, Desert,
        ];

        // Standard number tokens in a fixed order.
        let numbers: Vec<u8> = vec![5, 2, 6, 3, 8, 10, 9, 12, 11, 4, 8, 10, 9, 4, 5, 6, 3, 11];

        let mut number_iter = numbers.into_iter();
        let hexes: Vec<Hex> = coords
            .into_iter()
            .zip(terrains)
            .map(|(coord, terrain)| {
                let number_token = if terrain == Desert {
                    None
                } else {
                    Some(number_iter.next().unwrap())
                };
                Hex {
                    coord,
                    terrain,
                    number_token,
                }
            })
            .collect();

        let ports = standard_ports();
        Board { hexes, ports }
    }

    /// Generate a random standard Catan board.
    ///
    /// Uses rejection sampling to ensure that 6 and 8 tokens are never placed
    /// on adjacent hexes.
    pub fn generate(rng: &mut impl Rng) -> Board {
        let coords = board_hex_coords();

        loop {
            let mut terrains = standard_terrains();
            terrains.shuffle(rng);

            let mut tokens = standard_number_tokens();
            tokens.shuffle(rng);

            let mut token_iter = tokens.iter().copied();
            let hexes: Vec<Hex> = coords
                .iter()
                .zip(terrains.iter())
                .map(|(&coord, &terrain)| {
                    let number_token = if terrain == Terrain::Desert {
                        None
                    } else {
                        Some(token_iter.next().expect("not enough tokens"))
                    };
                    Hex {
                        coord,
                        terrain,
                        number_token,
                    }
                })
                .collect();

            if !has_adjacent_red_numbers(&hexes) {
                let ports = standard_ports();
                return Board { hexes, ports };
            }
            // Retry with a fresh shuffle.
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn make_board() -> Board {
        let mut rng = rand::thread_rng();
        Board::generate(&mut rng)
    }

    #[test]
    fn board_has_19_hexes() {
        let board = make_board();
        assert_eq!(board.hexes.len(), 19);
    }

    #[test]
    fn correct_resource_counts() {
        let board = make_board();
        let mut forest = 0;
        let mut hills = 0;
        let mut pasture = 0;
        let mut fields = 0;
        let mut mountains = 0;
        let mut desert = 0;
        for hex in &board.hexes {
            match hex.terrain {
                Terrain::Forest => forest += 1,
                Terrain::Hills => hills += 1,
                Terrain::Pasture => pasture += 1,
                Terrain::Fields => fields += 1,
                Terrain::Mountains => mountains += 1,
                Terrain::Desert => desert += 1,
            }
        }
        assert_eq!(forest, 4);
        assert_eq!(hills, 3);
        assert_eq!(pasture, 4);
        assert_eq!(fields, 4);
        assert_eq!(mountains, 3);
        assert_eq!(desert, 1);
    }

    #[test]
    fn desert_has_no_number_token() {
        let board = make_board();
        for hex in &board.hexes {
            if hex.terrain == Terrain::Desert {
                assert!(hex.number_token.is_none());
            } else {
                assert!(hex.number_token.is_some());
            }
        }
    }

    #[test]
    fn no_adjacent_6_8() {
        let board = make_board();
        assert!(!has_adjacent_red_numbers(&board.hexes));
    }

    #[test]
    fn correct_number_token_distribution() {
        let board = make_board();
        let mut tokens: Vec<u8> = board.hexes.iter().filter_map(|h| h.number_token).collect();
        tokens.sort();
        let mut expected = standard_number_tokens();
        expected.sort();
        assert_eq!(tokens, expected);
    }

    #[test]
    fn canonical_vertex_is_idempotent() {
        let coords = board_hex_coords();
        for c in &coords {
            for v in &hex_vertices(*c) {
                let c1 = canonical_vertex(*v);
                let c2 = canonical_vertex(c1);
                assert_eq!(c1, c2);
            }
        }
    }

    #[test]
    fn canonical_edge_is_idempotent() {
        let coords = board_hex_coords();
        for c in &coords {
            for e in &hex_edges(*c) {
                let c1 = canonical_edge(*e);
                let c2 = canonical_edge(c1);
                assert_eq!(c1, c2);
            }
        }
    }

    #[test]
    fn vertex_neighbors_return_2_or_3() {
        let board_set: HashSet<HexCoord> = board_hex_coords().into_iter().collect();
        let coords = board_hex_coords();
        let mut seen = HashSet::new();
        for c in &coords {
            for v in &hex_vertices(*c) {
                if seen.insert(*v) {
                    let neighbors = vertex_neighbors(*v);
                    let on_board: Vec<_> =
                        neighbors.iter().filter(|h| board_set.contains(h)).collect();
                    assert!(
                        on_board.len() >= 1 && on_board.len() <= 3,
                        "vertex {:?} has {} board neighbors",
                        v,
                        on_board.len()
                    );
                }
            }
        }
    }

    #[test]
    fn adjacent_vertices_return_3() {
        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        let adj = adjacent_vertices(v);
        assert_eq!(adj.len(), 3);
    }

    #[test]
    fn adjacent_vertices_are_all_distinct() {
        let coords = board_hex_coords();
        for c in &coords {
            for v in &hex_vertices(*c) {
                let adj = adjacent_vertices(*v);
                let set: HashSet<_> = adj.iter().collect();
                assert_eq!(
                    set.len(),
                    adj.len(),
                    "duplicate adjacent vertices for {:?}",
                    v
                );
            }
        }
    }

    #[test]
    fn hex_vertices_are_canonical() {
        let coords = board_hex_coords();
        for c in &coords {
            for v in &hex_vertices(*c) {
                assert_eq!(*v, canonical_vertex(*v), "vertex {:?} is not canonical", v);
            }
        }
    }

    #[test]
    fn hex_edges_are_canonical() {
        let coords = board_hex_coords();
        for c in &coords {
            for e in &hex_edges(*c) {
                assert_eq!(*e, canonical_edge(*e), "edge {:?} is not canonical", e);
            }
        }
    }

    #[test]
    fn edge_vertices_consistency() {
        let coords = board_hex_coords();
        for c in &coords {
            for e in &hex_edges(*c) {
                let (v1, v2) = edge_vertices(*e);
                let v1_edges = adjacent_edges(v1);
                let v2_edges = adjacent_edges(v2);
                assert!(
                    v1_edges.contains(e),
                    "edge {:?} not in adjacent_edges of vertex {:?}",
                    e,
                    v1
                );
                assert!(
                    v2_edges.contains(e),
                    "edge {:?} not in adjacent_edges of vertex {:?}",
                    e,
                    v2
                );
            }
        }
    }

    #[test]
    fn terrain_resource_mapping() {
        assert_eq!(Terrain::Forest.resource(), Some(Resource::Wood));
        assert_eq!(Terrain::Hills.resource(), Some(Resource::Brick));
        assert_eq!(Terrain::Pasture.resource(), Some(Resource::Sheep));
        assert_eq!(Terrain::Fields.resource(), Some(Resource::Wheat));
        assert_eq!(Terrain::Mountains.resource(), Some(Resource::Ore));
        assert_eq!(Terrain::Desert.resource(), None);
    }

    #[test]
    fn board_has_9_ports() {
        let board = make_board();
        assert_eq!(board.ports.len(), 9);
        for port in &board.ports {
            assert_eq!(port.port_type, PortType::Generic);
        }
    }

    #[test]
    fn board_hex_coords_are_correct() {
        let coords = board_hex_coords();
        assert_eq!(coords.len(), 19);
        assert!(coords.contains(&HexCoord::new(0, 0)));
        assert!(coords.contains(&HexCoord::new(2, -2)));
        assert!(coords.contains(&HexCoord::new(-2, 2)));
    }

    #[test]
    fn all_board_coords_pass_is_board_hex() {
        let coords = board_hex_coords();
        for c in &coords {
            assert!(is_board_hex(*c), "{:?} should be on the board", c);
        }
        assert!(!is_board_hex(HexCoord::new(3, 0)));
        assert!(!is_board_hex(HexCoord::new(0, 3)));
        assert!(!is_board_hex(HexCoord::new(-3, 0)));
    }

    #[test]
    fn hex_has_6_unique_vertices() {
        let coords = board_hex_coords();
        for c in &coords {
            let verts = hex_vertices(*c);
            let set: HashSet<_> = verts.iter().collect();
            assert_eq!(set.len(), 6, "hex {:?} doesn't have 6 unique vertices", c);
        }
    }

    #[test]
    fn hex_has_6_unique_edges() {
        let coords = board_hex_coords();
        for c in &coords {
            let edges = hex_edges(*c);
            let set: HashSet<_> = edges.iter().collect();
            assert_eq!(set.len(), 6, "hex {:?} doesn't have 6 unique edges", c);
        }
    }

    #[test]
    fn adjacent_vertices_via_edge_matches() {
        let v = VertexCoord::new(HexCoord::new(0, 0), VertexDirection::North);
        let adj_direct: HashSet<_> = adjacent_vertices(v).into_iter().collect();
        let adj_via_edges: HashSet<_> = adjacent_edges(v)
            .into_iter()
            .flat_map(|e| {
                let (v1, v2) = edge_vertices(e);
                vec![v1, v2]
            })
            .filter(|&vv| vv != v)
            .collect();
        assert_eq!(adj_direct, adj_via_edges);
    }

    #[test]
    fn generate_deterministic_with_seed() {
        use rand::SeedableRng;
        let mut rng1 = rand::rngs::StdRng::seed_from_u64(42);
        let mut rng2 = rand::rngs::StdRng::seed_from_u64(42);
        let b1 = Board::generate(&mut rng1);
        let b2 = Board::generate(&mut rng2);
        for (h1, h2) in b1.hexes.iter().zip(b2.hexes.iter()) {
            assert_eq!(h1.coord, h2.coord);
            assert_eq!(h1.terrain, h2.terrain);
            assert_eq!(h1.number_token, h2.number_token);
        }
    }

    #[test]
    fn adjacent_hexes_share_exactly_two_vertices() {
        let h1 = HexCoord::new(0, 0);
        for h2 in h1.neighbors() {
            let v1: HashSet<_> = h1.vertices().into_iter().collect();
            let v2: HashSet<_> = h2.vertices().into_iter().collect();
            let shared: Vec<_> = v1.intersection(&v2).collect();
            assert_eq!(
                shared.len(),
                2,
                "Adjacent hexes ({},{}) and ({},{}) should share 2 vertices, got {}",
                h1.q,
                h1.r,
                h2.q,
                h2.r,
                shared.len()
            );
        }
    }

    #[test]
    fn adjacent_hexes_share_exactly_one_edge() {
        let h1 = HexCoord::new(0, 0);
        for h2 in h1.neighbors() {
            let e1: HashSet<_> = h1.edges().into_iter().collect();
            let e2: HashSet<_> = h2.edges().into_iter().collect();
            let shared: Vec<_> = e1.intersection(&e2).collect();
            assert_eq!(
                shared.len(),
                1,
                "Adjacent hexes should share exactly 1 edge, got {}",
                shared.len()
            );
        }
    }

    #[test]
    fn board_all_vertices_count() {
        let board = Board::default_board();
        let verts = board.all_vertices();
        // Standard Catan board has 54 vertices.
        assert_eq!(verts.len(), 54, "Standard board should have 54 vertices");
    }

    #[test]
    fn board_all_edges_count() {
        let board = Board::default_board();
        let edges = board.all_edges();
        // Standard Catan board has 72 edges.
        assert_eq!(edges.len(), 72, "Standard board should have 72 edges");
    }

    #[test]
    fn north_vertex_neighbors_are_all_south() {
        let v = VertexCoord::new(HexCoord::new(1, -1), VertexDirection::North);
        for adj in adjacent_vertices(v) {
            assert_eq!(
                adj.dir,
                VertexDirection::South,
                "N vertex neighbors should all be S vertices"
            );
        }
    }

    #[test]
    fn south_vertex_neighbors_are_all_north() {
        let v = VertexCoord::new(HexCoord::new(1, -1), VertexDirection::South);
        for adj in adjacent_vertices(v) {
            assert_eq!(
                adj.dir,
                VertexDirection::North,
                "S vertex neighbors should all be N vertices"
            );
        }
    }

    #[test]
    fn terrain_abbr_covers_all_variants() {
        assert_eq!(Terrain::Forest.abbr(), "Wo");
        assert_eq!(Terrain::Hills.abbr(), "Bk");
        assert_eq!(Terrain::Pasture.abbr(), "Sh");
        assert_eq!(Terrain::Fields.abbr(), "Wh");
        assert_eq!(Terrain::Mountains.abbr(), "Or");
        assert_eq!(Terrain::Desert.abbr(), "De");
    }
}
