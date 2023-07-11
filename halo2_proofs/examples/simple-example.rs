use std::marker::PhantomData;

use halo2_proofs::{
    arithmetic::FieldExt,
    circuit::{AssignedCell, Chip, Layouter, Region, SimpleFloorPlanner, Value},
    plonk::{Advice, Circuit, Column, ConstraintSystem, Error, Fixed, Instance, Selector},
    poly::Rotation,
};

// ANCHOR: instructions
// 定义instruction，定义需要实现的方法接口：需要加载隐私输入和公共输入；涉及乘法需要mul方法。
trait NumericInstructions<F: FieldExt>: Chip<F> {
    /// Variable representing a number.
    /// 用于表示一个数的变量
    type Num;

    /// Loads a number into the circuit as a private input.
    /// 将一个数加载到电路中，用作隐私输入
    fn load_private(&self, layouter: impl Layouter<F>, a: Value<F>) -> Result<Self::Num, Error>;

    /// Loads a number into the circuit as a fixed constant.
    /// 将一个数加载到电路中，用作固定常数
    fn load_constant(&self, layouter: impl Layouter<F>, constant: F) -> Result<Self::Num, Error>;

    /// Returns `c = a * b`.
    /// 返回 `c = a * b`
    fn mul(
        &self,
        layouter: impl Layouter<F>,
        a: Self::Num,
        b: Self::Num,
    ) -> Result<Self::Num, Error>;

    /// Exposes a number as a public input to the circuit.
    /// 设置instance，将一个数作为电路的公开输入
    fn expose_public(
        &self,
        layouter: impl Layouter<F>,
        num: Self::Num,
        row: usize,
    ) -> Result<(), Error>;
}
// ANCHOR_END: instructions

// ANCHOR: chip
/// The chip that will implement our instructions! Chips store their own
/// config, as well as type markers if necessary.
/// 这块芯片将实现我们的指令集！芯片存储它们自己的配置，必要情况下也要包含type markers
struct FieldChip<F: FieldExt> {
    config: FieldConfig,
    _marker: PhantomData<F>,
}
// ANCHOR_END: chip

// ANCHOR: chip-config
/// Chip state is stored in a config struct. This is generated by the chip
/// during configuration, and then stored inside the chip.
/// 芯片的状态被存储在一个 config 结构体中，它是在配置过程中由芯片生成，并且存储在芯片内部。
///
/// 定义config。代码中chip指实现特定功能且可复用的模块，粒度可大可小。config中包含运算所需要的列。
#[derive(Clone, Debug)]
struct FieldConfig {
    /// For this chip, we will use two advice columns to implement our instructions.
    /// These are also the columns through which we communicate with other parts of
    /// the circuit.
    /// 对于这块芯片，我们将用到两个advice列来实现我们的指令集。
    /// 它们也是我们与电路的其他部分通信所需要用到列。
    advice: [Column<Advice>; 2],

    /// This is the public input (instance) column.
    /// 这是公开输入（instance）列
    instance: Column<Instance>,

    // We need a selector to enable the multiplication gate, so that we aren't placing
    // any constraints on cells where `NumericInstructions::mul` is not being used.
    // This is important when building larger circuits, where columns are used by
    // multiple sets of instructions.
    // 我们需要一个selector来激活乘法门，从而在用不到`NumericInstructions::mul`指令的
    // cells上不设置任何约束。这非常重要，尤其在构建更大型的电路的情况下，列会被多条指令集用到
    s_mul: Selector,
}

/// 实现chip，其中最重要的是configure方法，用来构造table column和gate约束。
impl<F: FieldExt> FieldChip<F> {
    fn construct(config: <Self as Chip<F>>::Config) -> Self {
        Self {
            config,
            _marker: PhantomData,
        }
    }

    fn configure(
        meta: &mut ConstraintSystem<F>,
        advice: [Column<Advice>; 2],
        instance: Column<Instance>,
        constant: Column<Fixed>,
    ) -> <Self as Chip<F>>::Config {
        meta.enable_equality(instance); // 传入参数的相等性检查
        meta.enable_constant(constant);
        for column in &advice {
            meta.enable_equality(*column);
        }
        let s_mul = meta.selector();

        // Define our multiplication gate!
        // 定义乘法门
        meta.create_gate("mul", |meta| {
            // To implement multiplication, we need three advice cells and a selector
            // cell. We arrange them like so:
            // 我们需要3个advice cells和1个selector cell来实现乘法
            // 我们把他们按下表来排列：
            //
            // | a0  | a1  | s_mul |
            // |-----|-----|-------|
            // | lhs | rhs | s_mul |
            // | out |     |       |
            //
            // Gates may refer to any relative offsets we want, but each distinct
            // offset adds a cost to the proof. The most common offsets are 0 (the
            // current row), 1 (the next row), and -1 (the previous row), for which
            // `Rotation` has specific constructors.
            // 门可以用任何相对偏移，但每一个不同的偏移都会对证明增加开销。
            // 最常见的偏移值是 0 (当前行), 1(下一行), -1(上一行)。
            // 针对这三种情况，有特定的构造函数来构造`Rotation`结构。
            let lhs = meta.query_advice(advice[0], Rotation::cur());
            let rhs = meta.query_advice(advice[1], Rotation::cur());
            let out = meta.query_advice(advice[0], Rotation::next());
            let s_mul = meta.query_selector(s_mul);

            // Finally, we return the polynomial expressions that constrain this gate.
            // For our multiplication gate, we only need a single polynomial constraint.
            // 最终，我们将约束门的多项式表达式返回。
            // 对于我们的乘法门，我们仅需要一个多项式约束。
            //
            // The polynomial expressions returned from `create_gate` will be
            // constrained by the proving system to equal zero. Our expression
            // has the following properties:
            // The polynomial expressions returned from `create_gate` will be constrained by the proving system to equal zero. Our expression has the following properties:
            // - When s_mul = 0, any value is allowed in lhs, rhs, and out.
            // - When s_mul != 0, this constrains lhs * rhs = out.
            // `create_gate`函数返回的多项式表达式，在证明系统中一定等于0。
            // 我们的表达式有以下性质：
            // - 当s_mul = 0时，lhs、rhs、out可以是任意值。
            // - 当s_mul != 0时，lhs、rhs、out将满足lhs * rhs = out这条约束。
            vec![s_mul * (lhs * rhs - out)]
        });

        FieldConfig {
            advice,
            instance,
            s_mul,
        }
    }
}
// ANCHOR_END: chip-config

// ANCHOR: chip-impl
/// 每一个"芯片"类型都要实现Chip接口。
/// Chip接口定义了Layouter在做电路综合时可能需要的关于电路的某些属性，
/// 以及若将该芯片加载到电路所需要设置的任何初始状态。
impl<F: FieldExt> Chip<F> for FieldChip<F> {
    type Config = FieldConfig;
    type Loaded = ();

    /// 返回自定义chip的配置
    fn config(&self) -> &Self::Config {
        &self.config
    }

    /// 返回自定义chip的载入数据
    fn loaded(&self) -> &Self::Loaded {
        &()
    }
}
// ANCHOR_END: chip-impl

// ANCHOR: instructions-impl
/// A variable representing a number.
/// 用于表示数的变量
#[derive(Clone)]
struct Number<F: FieldExt>(AssignedCell<F, F>);

/// 对chip实现第一步instruction定义的接口
impl<F: FieldExt> NumericInstructions<F> for FieldChip<F> {
    type Num = Number<F>;

    // 加载witness
    fn load_private(
        &self,
        mut layouter: impl Layouter<F>,
        value: Value<F>,
    ) -> Result<Self::Num, Error> {
        let config = self.config();

        layouter.assign_region(
            || "load private",
            |mut region| {
                region
                    .assign_advice(|| "private input", config.advice[0], 0, || value)
                    .map(Number)
            },
        )
    }

    fn load_constant(
        &self,
        mut layouter: impl Layouter<F>,
        constant: F,
    ) -> Result<Self::Num, Error> {
        let config = self.config();

        layouter.assign_region(
            || "load constant",
            |mut region| {
                region
                    .assign_advice_from_constant(|| "constant value", config.advice[0], 0, constant)
                    .map(Number)
            },
        )
    }

    fn mul(
        &self,
        mut layouter: impl Layouter<F>,
        a: Self::Num,
        b: Self::Num,
    ) -> Result<Self::Num, Error> {
        let config = self.config();

        layouter.assign_region(
            || "mul",
            |mut region: Region<'_, F>| {
                // We only want to use a single multiplication gate in this region,
                // so we enable it at region offset 0; this means it will constrain
                // cells at offsets 0 and 1.
                // 在这个region中，我们只想用一个乘法门，所以我们在region偏移0处，
                // 激活它；这意味着它将对0偏移和1偏移处的两个cells进行约束。
                config.s_mul.enable(&mut region, 0)?;

                // The inputs we've been given could be located anywhere in the circuit,
                // but we can only rely on relative offsets inside this region. So we
                // assign new cells inside the region and constrain them to have the
                // same values as the inputs.
                // 给我们的输入有可能在电路的任一位置，但在当前region中，我们仅可以用相对偏移。
                // 所以，我们在region内分配新的cells并限定他们的值与输入cells的值相等。
                a.0.copy_advice(|| "lhs", &mut region, config.advice[0], 0)?;
                b.0.copy_advice(|| "rhs", &mut region, config.advice[1], 0)?;

                // Now we can assign the multiplication result, which is to be assigned
                // into the output position.
                // 现在我们可以赋值乘法结果，将赋值到输出位置。
                let value = a.0.value().copied() * b.0.value();

                // Finally, we do the assignment to the output, returning a
                // variable to be used in another part of the circuit.
                // 最后，我们对输出进行赋值，返回一个要在电路的另一部分中被使用的变量。
                region
                    .assign_advice(|| "lhs * rhs", config.advice[0], 1, || value)
                    .map(Number)
            },
        )
    }

    // 设置instance列
    fn expose_public(
        &self,
        mut layouter: impl Layouter<F>,
        num: Self::Num,
        row: usize,
    ) -> Result<(), Error> {
        let config = self.config();

        layouter.constrain_instance(num.0.cell(), config.instance, row)
    }
}
// ANCHOR_END: instructions-impl

// ANCHOR: circuit
/// The full circuit implementation.
///
/// In this struct we store the private input variables. We use `Option<F>` because
/// they won't have any value during key generation. During proving, if any of these
/// were `None` we would get an error.
/// 完整的电路实现
/// 在这个结构体中，我们保存隐私输入变量。我们使用`Option<F>`类型是因为它们在生成密钥阶段不需要有任何的值。
/// 在证明阶段中，如果它们任一为`None`的话，我们将得到一个错误。
///
/// 使用实现的chip构造电路
#[derive(Default)]
struct MyCircuit<F: FieldExt> {
    constant: F,
    a: Value<F>,
    b: Value<F>,
}

impl<F: FieldExt> Circuit<F> for MyCircuit<F> {
    // Since we are using a single chip for everything, we can just reuse its config.
    // 因为我们在任一地方值用了一个芯片，所以我们可以重用它的配置。
    type Config = FieldConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self::default()
    }

    fn configure(meta: &mut ConstraintSystem<F>) -> Self::Config {
        // We create the two advice columns that FieldChip uses for I/O.
        // 我们创建两个advice列，作为FieldChip的输入。
        let advice = [meta.advice_column(), meta.advice_column()];

        // We also need an instance column to store public inputs.
        // 我们还需要一个instance列来存储公开输入。
        let instance = meta.instance_column();

        // Create a fixed column to load constants.
        // 创建一个fixed列来加载常数
        let constant = meta.fixed_column();

        FieldChip::configure(meta, advice, instance, constant)
    }

    fn synthesize(
        &self,
        config: Self::Config,
        mut layouter: impl Layouter<F>,
    ) -> Result<(), Error> {
        let field_chip = FieldChip::<F>::construct(config);

        // Load our private values into the circuit.
        // 将我们的隐私值加载到电路中。
        let a = field_chip.load_private(layouter.namespace(|| "load a"), self.a)?;
        let b = field_chip.load_private(layouter.namespace(|| "load b"), self.b)?;

        // Load the constant factor into the circuit.
        // 将常数因子加载到电路中。
        let constant =
            field_chip.load_constant(layouter.namespace(|| "load constant"), self.constant)?;

        // We only have access to plain multiplication.
        // We could implement our circuit as:
        // 我们只能使用简单的乘法。我们可以将我们的电路实现为：
        //     asq  = a*a
        //     bsq  = b*b
        //     absq = asq*bsq
        //     c    = constant*asq*bsq
        //
        // but it's more efficient to implement it as:
        // 但按以下实现更为高效：
        //     ab   = a*b
        //     absq = ab^2
        //     c    = constant*absq
        let ab = field_chip.mul(layouter.namespace(|| "a * b"), a, b)?;
        let absq = field_chip.mul(layouter.namespace(|| "ab * ab"), ab.clone(), ab)?;
        let c = field_chip.mul(layouter.namespace(|| "constant * absq"), constant, absq)?;

        // Expose the result as a public input to the circuit.
        // 将结果公开为电路的公开输入。
        field_chip.expose_public(layouter.namespace(|| "expose c"), c, 0)
    }
}
// ANCHOR_END: circuit

fn main() {
    use halo2_proofs::dev::MockProver;
    use halo2curves::pasta::Fp;

    // ANCHOR: test-circuit
    // The number of rows in our circuit cannot exceed 2^k. Since our example
    // circuit is very small, we can pick a very small value here.
    // 我们电路的行数不能超过2^k。由于我们的示例电路很小，我们可以选择一个较小的值。
    let k = 4;

    // Prepare the private and public inputs to the circuit!
    // 准备好电路的隐私输入和公开输入
    let constant = Fp::from(7);
    let a = Fp::from(2);
    let b = Fp::from(3);
    let c = constant * a.square() * b.square();

    // Instantiate the circuit with the private inputs.
    // 用隐私输入来实例化电路
    let circuit = MyCircuit {
        constant,
        a: Value::known(a),
        b: Value::known(b),
    };

    // Arrange the public input. We expose the multiplication result in row 0
    // of the instance column, so we position it there in our public inputs.
    // 将公开输入进行排列。乘法的结果被我们放置在instance列的第0行，所以我们把它放在公开输入的对应位置。
    let mut public_inputs = vec![c];

    // Given the correct public input, our circuit will verify.
    // 给定正确的公开输入，我们的电路能验证通过
    let prover = MockProver::run(k, &circuit, vec![public_inputs.clone()]).unwrap();
    println!("prover1:\n{:?}", prover);
    assert_eq!(prover.verify(), Ok(()));

    use plotters::prelude::*;
    let root = BitMapBackend::new("simple-example-layout.png", (1024, 768)).into_drawing_area();
    root.fill(&WHITE).unwrap();
    let root = root
        .titled("Simple Example Circuit Layout", ("sans-serif", 24))
        .unwrap();

    halo2_proofs::dev::CircuitLayout::default()
        // You can optionally render only a section of the circuit.
        .view_width(0..6)
        .view_height(0..11)
        // You can hide labels, which can be useful with smaller areas.
        .show_labels(true)
        // Render the circuit onto your area!
        // The first argument is the size parameter for the circuit.
        .render(5, &circuit, &root)
        .unwrap();

    // If we try some other public input, the proof will fail!
    // 如果我们尝试用其他的公开输入，证明将失败！
    public_inputs[0] += Fp::one();
    let prover = MockProver::run(k, &circuit, vec![public_inputs]).unwrap();
    assert!(prover.verify().is_err());
}
