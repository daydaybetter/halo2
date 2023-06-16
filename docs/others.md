## Chip、 gadget 和 region

Halo2中电路的主要构建块是[Gadget和Chip](https://docs.rs/halo2_gadgets/0.2.0/halo2_gadgets/)。Chip是最低级别的单位。Chip通常会公开一种配置门的方法，以及在合成过程为单元（cell）赋值的多种方法。还可以构建由其他Chip组成的Chip。另一方面，**Gadget**在更高的抽象级别上运行，隐藏了底层Chip的实现细节，尽管可以直接用Chip构建电路并完全跳过 gadget。

为了提高可重用性，Chip总是在相对偏移量上运行。这允许将多个Chip分组到电路中的不同[Region](https://docs.rs/halo2_proofs/0.2.0/halo2_proofs/circuit/trait.Layouter.html#tymethod.assign_region)。一旦定义了所有region及其形状，[FloorPlanner](https://docs.rs/halo2_proofs/0.2.0/halo2_proofs/plonk/trait.FloorPlanner.html)就会在矩阵上排列这些Region，因此无需直接定义每个Chip的实际放置位置。然而，根据构建电路的方式，完全有可能将所有内容打包到一个Region中，而不是将布局委托给Planner。

Halo2 Rust API

在Halo2中，代码将在不同情况下被多次调用：无论是配置矩阵、生成约束、创建证明还是计算见证。

你的电路需要实现一个特定的[`Circuit` trait](https://docs.rs/halo2_proofs/0.2.0/halo2_proofs/plonk/trait.Circuit.html)用来定义方法，在整个生命周期中调用，可以是具体的，也可以是未知[`Value`](https://docs.rs/halo2_proofs/0.2.0/halo2_proofs/circuit/struct.Value.html)：
```rust
/// This is a trait that circuits provide implementations for so that the
/// backend prover can ask the circuit to synthesize using some given
/// [`ConstraintSystem`] implementation.
pub trait Circuit<F: Field> {
    /// This is a configuration object that stores things like columns.
    type Config: Clone;
    /// The floor planner used for this circuit. This is an associated type of the
    /// `Circuit` trait because its behaviour is circuit-critical.
    type FloorPlanner: FloorPlanner;

    /// Returns a copy of this circuit with no witness values (i.e. all witnesses set to
    /// `None`). For most circuits, this will be equal to `Self::default()`.
    fn without_witnesses(&self) -> Self;

    /// The circuit is given an opportunity to describe the exact gate
    /// arrangement, column arrangement, etc.
    fn configure(meta: &mut ConstraintSystem<F>) -> Self::Config;

    /// Given the provided `cs`, synthesize the circuit. The concrete type of
    /// the caller will be different depending on the context, and they may or
    /// may not expect to have a witness present.
    fn synthesize(&self, config: Self::Config, layouter: impl Layouter<F>) -> Result<(), Error>;
}
```
这里的重要部分是`configure`和`synthesize`。简单来说是`configure`设置矩阵形状和它的门，`synthesize`计算见证并使用相应的值填充矩阵。但是，`synthesize`也可能在其他阶段被未知值调用，因此你需要始终使用包装好的[`Value`](https://docs.rs/halo2_proofs/0.2.0/halo2_proofs/circuit/struct.Value.html)。例如，虽然看起来有悖常理，但门中的多项式约束是在配置期间定义的，而等式约束是在`synthesize`设置的。

Halo2书中的[示例](https://zcash.github.io/halo2/user/simple-example.html)很好地逐步介绍了如何使用单Chip实现简单电路，以防还需要一个脚手架。
















石头剪刀布的 Halo2 实现
石头剪刀布的 Halo2 实现[113]比 Circom 要冗长得多，我们在这里只重现一些部分。首先，矩阵设置有以下列：

两个玩家各自的 advice 列，xs和ys，行中的值i表示玩家在回合i中的选择。
第三个 advice 列accum，跟踪累积总分，因此行i包含之前所有回合的分数总和.
一个公共实例列，最后一个单元（cell）被限制为等于accum的值，因此只显示总分而不显示中间分。
一个选择器（selector），用于启用验证输入并计算每轮分数的单个门。
主chip[114]定义了一个自定义门，它包含每一轮的所有约束：验证输入是否在范围内，计算该轮的分数并将其添加到累计总分中。该 chip 用一行中xs、ys和accum的值作为“输入”，并在下一行的列accum中“输出”新的分数。
```rust
meta.create_gate("round", |meta| {  
    let s = meta.query_selector(selector);  
  
    let x = meta.query_advice(col_x, Rotation::cur());  
    let y = meta.query_advice(col_y, Rotation::cur());  
    let accum = meta.query_advice(col_accum, Rotation::cur());  
  
    // We store the output in the accum column in the next row  
    let out = meta.query_advice(col_accum, Rotation::next());  
  
    // Constraints for each round  
    vec![  
        // out = y_wins * 6 + is_draw * 3 + y + 1 + accum  
        s.clone() * (out - (y_wins.expr() * F::from(6) + is_draw.expr() * F::from(3) + y.clone() + const_val(1) + accum)),  
        // x in (0,1,2)  
        s.clone() * x.clone() * (x.clone() - const_val(1)) * (x.clone() - const_val(2)),  
        // y in (0,1,2)  
        s.clone() * y.clone() * (y.clone() - const_val(1)) * (y.clone() - const_val(2)),  
    ]  
});  
```
上面的y_wins和is_draw是如下定义的IsZero chip 。请注意，我们可以所有约束使用相同的选择器列，因为没有哪一行需要启用某些约束禁用其他约束。
```rust
// yWins <==> (y+2-x) * (y-1-x) == 0;  
let y_wins = IsZeroChip::configure(  
    meta,  
    |meta| meta.query_selector(selector),   
    |meta| {  
        let x = meta.query_advice(col_x, Rotation::cur());  
        let y = meta.query_advice(col_y, Rotation::cur());  
        (y.clone() + const_val(2) - x.clone()) * (y - const_val(1) - x)  
    }  
);  
```
在整合电路时，循环遍历每一轮输入，计算累积分数，并将计算出的值分配给矩阵。注意，对于“执行”模式，我们可以直接使用条件表达式来计算分数。
```rust
// Assign one row per round  
for row in 0..N {  
  let [x, y] = [xs[row], ys[row]];   
  
  // Enable the selector for the round gate  
  self.config.selector.enable(&mut region, row)?;  
  
  // This is requiring us to add a constant column to the chip config just with zeros  
  if row == 0 {  
      region.assign_advice_from_constant(|| "zero", col_accum, 0, F::ZERO)?;  
  }  
  
  // Set x and y advice columns to the input values  
  region.assign_advice(|| format!("x[{}]", row),col_x,row,|| x)?;  
  region.assign_advice(|| format!("y[{}]", row),col_y,row,|| y)?;  
  
  // Assign the is_zero chips to the same expressions defined in the gates  
  // yWins <==> (y+2-x) * (y-1-x) == 0;  
  let y_wins_chip = IsZeroChip::construct(y_wins.clone());  
  let y_wins_value = (y + Value::known(F::from(2)) - x) * (y - Value::known(F::ONE) - x);  
  let y_wins = y_wins_chip.assign(&mut region, row, y_wins_value)?;  
  
  // isDraw <==> y-x == 0;  
  let is_draw_chip = IsZeroChip::construct(is_draw.clone());  
  let is_draw_value = y - x;  
  let is_draw = is_draw_chip.assign(&mut region, row, is_draw_value)?;  
  
  // Calculate the score of this round  
  let round_score = y_wins.zip(is_draw).and_then(|(y_wins, is_draw)| {  
      let partial_score = if y_wins { 6 } else if is_draw { 3 } else { 0 };  
      Value::known(F::from(partial_score)) + y + Value::known(F::ONE)  
  });  
  
  // Assign the col_accum *in the next row* to the new score  
  accum_value = accum_value + round_score;  
  out_cell = region.assign_advice(  
      || format!("out[{}]", row),  
      col_accum,  
      row + 1,  
      || accum_value  
  );  
};  
```
最后一位是通过约束实例列[115]来匹配矩阵最后一行中的列accum，将总分作为电路的公共输出：

```rust
layouter.constrain_instance(out_cell.cell(), self.config.instance, N-1)  
```
















理解Halo2，可以从两部分着手：1/ 电路构建 2/ 证明系统。从开发者的角度看，电路构建是接口。如何通过Halo2创建电路，这些电路在Halo2的内部如何表示是理解电路构建的关键。Halo2中的Chip电路由一个个Region组成，在Halo2的框架中，Region通过Layouter进行分配。电路的所有的信息都存储在Assignment的接口中。Halo2的电路构建分为两部分：1/Configure （电路配置）2/ Synthesize（电路综合）。简单的说，Configure就是进行电路本身的配置。Synthesize进行某个电路实例的综合。
