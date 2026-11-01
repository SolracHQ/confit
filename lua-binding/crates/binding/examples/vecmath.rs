//! Vector math over Lua userdata, operators, fields, and a module table.
//!
//! The example installs a `vecmath` module and a mutable `counter` userdata,
//! then drives every bound shape from a Lua script.

use lua_binding::generator::Generator;
use lua_binding::stub::Emitter;
use lua_binding::value::Encode;
use lua_binding::{Shape, lua_methods, lua_module};
use mlua::Lua;

/// The dimension of every transform matrix.
const TRANSFORM_DIMENSION: usize = 3;

/// A square transform matrix.
type Transform = [[f64; TRANSFORM_DIMENSION]; TRANSFORM_DIMENSION];

/// A two-dimensional vector stored as Lua userdata.
#[derive(Clone, Shape)]
#[shape(object)]
pub struct Vec2 {
    /// The x coordinate.
    #[shape(get, set)]
    pub x: f64,
    /// The y coordinate.
    #[shape(get, set)]
    pub y: f64,
}

#[lua_methods]
impl Vec2 {
    /// Returns the Euclidean length.
    ///
    /// # Returns
    ///
    /// - The vector length.
    fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Returns the dot product with another vector.
    ///
    /// # Arguments
    ///
    /// - [other] The second vector.
    ///
    /// # Returns
    ///
    /// - The dot product.
    fn dot(&self, other: Vec2) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// Returns the vector scaled to unit length.
    ///
    /// # Returns
    ///
    /// - The unit vector.
    fn normalize(&self) -> Vec2 {
        let length = self.length();
        Vec2 {
            x: self.x / length,
            y: self.y / length,
        }
    }

    /// Adds another vector.
    ///
    /// # Arguments
    ///
    /// - [other] The vector to add.
    ///
    /// # Returns
    ///
    /// - The sum vector.
    #[operator(__add)]
    fn add(&self, other: Vec2) -> Vec2 {
        Vec2 {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }

    /// Subtracts another vector.
    ///
    /// # Arguments
    ///
    /// - [other] The vector to subtract.
    ///
    /// # Returns
    ///
    /// - The difference vector.
    #[operator(__sub)]
    fn sub(&self, other: Vec2) -> Vec2 {
        Vec2 {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }

    /// Scales the vector by a factor.
    ///
    /// # Arguments
    ///
    /// - [factor] The scale factor.
    ///
    /// # Returns
    ///
    /// - The scaled vector.
    #[operator(__mul)]
    fn mul(&self, factor: f64) -> Vec2 {
        Vec2 {
            x: self.x * factor,
            y: self.y * factor,
        }
    }

    /// Compares two vectors.
    ///
    /// # Arguments
    ///
    /// - [other] The vector to compare.
    ///
    /// # Returns
    ///
    /// - Whether both coordinates match.
    #[operator(__eq)]
    fn eq(&self, other: Vec2) -> bool {
        self.x == other.x && self.y == other.y
    }

    /// Renders the vector for `tostring`.
    ///
    /// # Returns
    ///
    /// - The rendered vector.
    #[operator(__tostring)]
    fn display(&self) -> String {
        format!("vec2({}, {})", self.x, self.y)
    }
}

/// A three-dimensional vector stored as Lua userdata.
#[derive(Clone, Shape)]
#[shape(object)]
pub struct Vec3 {
    /// The x coordinate.
    #[shape(get, set)]
    pub x: f64,
    /// The y coordinate.
    #[shape(get, set)]
    pub y: f64,
    /// The z coordinate.
    #[shape(get, set)]
    pub z: f64,
}

#[lua_methods]
impl Vec3 {
    /// Returns the Euclidean length.
    ///
    /// # Returns
    ///
    /// - The vector length.
    fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// Returns the dot product with another vector.
    ///
    /// # Arguments
    ///
    /// - [other] The second vector.
    ///
    /// # Returns
    ///
    /// - The dot product.
    fn dot(&self, other: Vec3) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Returns the cross product with another vector.
    ///
    /// # Arguments
    ///
    /// - [other] The second vector.
    ///
    /// # Returns
    ///
    /// - The cross product.
    fn cross(&self, other: Vec3) -> Vec3 {
        Vec3 {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    /// Returns the vector scaled to unit length.
    ///
    /// # Returns
    ///
    /// - The unit vector.
    fn normalize(&self) -> Vec3 {
        let length = self.length();
        Vec3 {
            x: self.x / length,
            y: self.y / length,
            z: self.z / length,
        }
    }

    /// Adds another vector.
    ///
    /// # Arguments
    ///
    /// - [other] The vector to add.
    ///
    /// # Returns
    ///
    /// - The sum vector.
    #[operator(__add)]
    fn add(&self, other: Vec3) -> Vec3 {
        Vec3 {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }

    /// Subtracts another vector.
    ///
    /// # Arguments
    ///
    /// - [other] The vector to subtract.
    ///
    /// # Returns
    ///
    /// - The difference vector.
    #[operator(__sub)]
    fn sub(&self, other: Vec3) -> Vec3 {
        Vec3 {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }

    /// Scales the vector by a factor.
    ///
    /// # Arguments
    ///
    /// - [factor] The scale factor.
    ///
    /// # Returns
    ///
    /// - The scaled vector.
    #[operator(__mul)]
    fn mul(&self, factor: f64) -> Vec3 {
        Vec3 {
            x: self.x * factor,
            y: self.y * factor,
            z: self.z * factor,
        }
    }

    /// Compares two vectors.
    ///
    /// # Arguments
    ///
    /// - [other] The vector to compare.
    ///
    /// # Returns
    ///
    /// - Whether all coordinates match.
    #[operator(__eq)]
    fn eq(&self, other: Vec3) -> bool {
        self.x == other.x && self.y == other.y && self.z == other.z
    }

    /// Renders the vector for `tostring`.
    ///
    /// # Returns
    ///
    /// - The rendered vector.
    #[operator(__tostring)]
    fn display(&self) -> String {
        format!("vec3({}, {}, {})", self.x, self.y, self.z)
    }
}

/// A mutable counter stored as Lua userdata.
#[derive(Clone, Shape)]
#[shape(object)]
pub struct Counter {
    /// The current count.
    #[shape(get, set)]
    pub count: i64,
    /// The counter name.
    #[shape(get)]
    pub name: String,
}

#[lua_methods]
impl Counter {
    /// Increments the count.
    fn bump(&mut self) {
        self.count += 1;
    }

    /// Returns the current count.
    ///
    /// # Returns
    ///
    /// - The current count.
    #[operator(__len)]
    fn length(&self) -> i64 {
        self.count
    }

    /// Renders the counter for `tostring`.
    ///
    /// # Returns
    ///
    /// - The rendered counter.
    #[operator(__tostring)]
    fn display(&self) -> String {
        format!("Counter({})", self.count)
    }
}

/// The `vecmath` module table.
struct Vecmath;

#[lua_module("vecmath")]
impl Vecmath {
    /// Builds a 2D vector.
    ///
    /// # Arguments
    ///
    /// - [x] The x coordinate.
    /// - [y] The y coordinate.
    ///
    /// # Returns
    ///
    /// - The new vector.
    fn vec2(x: f64, y: f64) -> Vec2 {
        Vec2 { x, y }
    }

    /// Builds a 3D vector.
    ///
    /// # Arguments
    ///
    /// - [x] The x coordinate.
    /// - [y] The y coordinate.
    /// - [z] The z coordinate.
    ///
    /// # Returns
    ///
    /// - The new vector.
    fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// Interpolates between two vectors.
    ///
    /// # Arguments
    ///
    /// - [a] The start vector.
    /// - [b] The end vector.
    /// - [t] The interpolation weight.
    ///
    /// # Returns
    ///
    /// - The interpolated vector.
    fn lerp(a: Vec2, b: Vec2, t: f64) -> Vec2 {
        Vec2 {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
        }
    }

    /// Returns the identity transform.
    ///
    /// # Returns
    ///
    /// - The identity matrix.
    fn identity() -> Transform {
        let mut matrix = [[0.0; TRANSFORM_DIMENSION]; TRANSFORM_DIMENSION];
        for (index, row) in matrix.iter_mut().enumerate() {
            row[index] = 1.0;
        }
        matrix
    }

    /// Rotates a transform by an angle in radians.
    ///
    /// # Arguments
    ///
    /// - [t] The source transform.
    /// - [angle] The rotation angle in radians.
    ///
    /// # Returns
    ///
    /// - The rotated transform.
    fn rotate(t: Transform, angle: f64) -> Transform {
        let cos = angle.cos();
        let sin = angle.sin();
        let mut rotated = [[0.0; TRANSFORM_DIMENSION]; TRANSFORM_DIMENSION];
        for (row, source) in t.iter().enumerate() {
            rotated[row][0] = source[0] * cos + source[1] * sin;
            rotated[row][1] = -source[0] * sin + source[1] * cos;
            rotated[row][2] = source[2];
        }
        rotated
    }

    /// Scales a transform by a factor.
    ///
    /// # Arguments
    ///
    /// - [t] The source transform.
    /// - [factor] The scale factor.
    ///
    /// # Returns
    ///
    /// - The scaled transform.
    fn scale(t: Transform, factor: f64) -> Transform {
        let mut scaled = t;
        for row in &mut scaled {
            for value in row {
                *value *= factor;
            }
        }
        scaled
    }

    /// Applies a transform to a vector.
    ///
    /// # Arguments
    ///
    /// - [t] The transform.
    /// - [v] The vector to transform.
    ///
    /// # Returns
    ///
    /// - The transformed vector.
    fn apply(t: Transform, v: Vec2) -> Vec2 {
        Vec2 {
            x: t[0][0] * v.x + t[0][1] * v.y + t[0][2],
            y: t[1][0] * v.x + t[1][1] * v.y + t[1][2],
        }
    }
}

fn main() -> mlua::Result<()> {
    let lua = Lua::new();
    let module = Vecmath::add(&lua)?;
    let package: mlua::Table = lua.globals().get("package")?;
    let preload: mlua::Table = package.get("preload")?;
    preload.set(
        "vecmath",
        lua.create_function(move |_, ()| Ok(module.clone()))?,
    )?;

    let counter = Counter {
        count: 0,
        name: "visits".to_owned(),
    }
    .encode(&lua)?;
    lua.globals().set("counter", counter)?;

    lua.load(
        r##"
        local vecmath = require("vecmath")
        local a = vecmath.vec2(3, 4)
        local b = vecmath.vec2(1, 2)

        assert(a:length() == 5, "length")
        assert(a:dot(b) == 11, "dot")

        local sum = a + b
        assert(sum.x == 4 and sum.y == 6, "add")

        local difference = a - b
        assert(difference.x == 2 and difference.y == 2, "sub")

        local scaled = a * 2
        assert(scaled.x == 6 and scaled.y == 8, "mul")

        assert(a == vecmath.vec2(3, 4), "equal vectors")
        assert(a ~= b, "different vectors")
        assert(tostring(a) == "vec2(3, 4)", "vec2 tostring")

        local unit = a:normalize()
        assert(math.abs(unit:length() - 1) < 1e-12, "normalize")

        local middle = vecmath.lerp(a, b, 0.5)
        assert(middle.x == 2 and middle.y == 3, "lerp")

        local cross = vecmath.vec3(1, 0, 0):cross(vecmath.vec3(0, 1, 0))
        assert(cross.x == 0 and cross.y == 0 and cross.z == 1, "cross")

        local one = vecmath.vec3(1, 2, 3)
        local two = vecmath.vec3(4, 5, 6)
        local total = one + two
        assert(total.x == 5 and total.y == 7 and total.z == 9, "vec3 add")
        assert(one:dot(two) == 32, "vec3 dot")
        assert(math.abs(vecmath.vec3(2, 3, 6):length() - 7) < 1e-12, "vec3 length")
        assert(tostring(one) == "vec3(1, 2, 3)", "vec3 tostring")

        local transformed = vecmath.apply(
            vecmath.rotate(vecmath.scale(vecmath.identity(), 2), math.pi / 4),
            vecmath.vec2(3, 4)
        )
        assert(
            math.abs(transformed.x - (-1.4142135623730951)) < 1e-9,
            "transform x"
        )
        assert(
            math.abs(transformed.y - 9.899494936611665) < 1e-9,
            "transform y"
        )

        assert(counter.count == 0, "counter starts at zero")
        counter.count = 5
        counter:bump()
        assert(counter.count == 6, "counter bump")
        assert(#counter == 6, "counter length")
        assert(not pcall(function() counter.name = "x" end), "read-only name")
        assert(tostring(counter) == "Counter(6)", "counter tostring")
        "##,
    )
    .exec()?;

    let stub = Vecmath::stub();
    assert!(stub.text.contains("--- @class Vec2"));
    assert!(stub.text.contains("--- @field x number"));
    assert!(stub.text.contains("--- @operator add(Vec2): Vec2"));
    assert!(stub.text.contains("--- @operator mul(number): Vec2"));
    assert!(stub.text.contains("function Vec2:length()"));
    assert!(stub.text.contains("function vec2(x, y) end"));

    let mut generator = Generator::new();
    Counter::collect(&mut generator);
    let counter_stub = Emitter::new("--- @meta counter").emit("counter", &generator);
    assert!(counter_stub.text.contains("--- @field name string"));
    assert!(counter_stub.text.contains("--- @field count integer"));
    assert!(counter_stub.text.contains("--- @operator len: integer"));

    println!("vecmath example passed");
    Ok(())
}
