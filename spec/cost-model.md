---
title: Cost Model Architecture
author: Thomas Kerber
date: 2025-08-28
---
# Cost Model Architecture

This document describes the methodology used to build the ledger's cost model.

## Problem Statement

The ledger must charge for transactions to deter denial of service attacks.
There is a tension that encourages these prices to be as accurate as possible:

- A practical desire to keep transaction fees low
- A security requirement to not under-charge

This is further complicated by the variety of different host systems that may
run a node, and there being a number of unrelated 'costs' that need to be
balanced.

## Adopted Solution

The ledger has adopted a multi-dimensional cost model, with I/O and CPU costs
tracked separately.
Each of these come with subtleties, that we will go into. Further, we
distinguish the cost of *validating* and *applying* transactions, each is its
own independent bucket to capture costs under. This not only matches
measurements better, but also allows us to take advantage of the commutative
nature of validation.

Core to this is *what* we consider to be resource costs. There are four
*primary* costs, and one second-order cost:

1. Compute time, both validating and applying transactions.
2. I/O read time, accessing data required for 1.
3. Consensus throughput usage.
4. Persistent disk storage
5. (secondary) "Churn", or temporary disk storage

The first two are measured in *time* (specifically, picoseconds), while the
latter three are measure in *bytes*. It may seem counterintuitive that disk
reads are measured in time, while writes are measured in quantity -- the reason
for this is straightforward: For reads, we only care that we can read all the
data we need in order to process a block within its timing constraints. Reads benefit
from locality, and from batching; the access pattern matters more than the
amount. Conversely, writes can always use an append log if speed is essential,
but more importantly, it is clear that storage would quickly become the
bottleneck of any system that saturates its *write* throughput. The bound for
writes is more importantly a bound of *storage*, not the action of writing.

For compute time, we will explicitly consider *single-threaded* compute time.
This is to simplify the low-level model, while still allowing parallelism
between transactions to take advantage of multithreading.

Concretely, we define a synthetic cost as:

```rust
// Modelled time (in picoseconds)
struct Duration(u64);

struct SyntheticCost {
    // The total time, in seconds, budgeted for synthetic I/O read time.
    pub read_time: Duration,
    // The total time, in seconds.
    // At the transaction level, budgeted for single-threaded compute.
    // At the block level, budgeted for multi-threaded compute.
    pub compute_time: Duration,
    // The block usage in bytes.
    pub block_usage: u64,
    // The I/O bytes written
    pub bytes_written: u64,
    // The I/O bytes churned
    pub bytes_churned: u64,
}
```

### Churn vs Writes

For the time being, our model of writes only distinguishes between two 'levels'
of writes: those that *may* stay stored indefinitely, and those that *will* or
*already have* been marked for deletion. This simplifies computation somewhat,
as it is sufficient to track:

* The number of bytes newly allocated into storage
* The number of bytes deallocated from storage

In other words, the positive and negative deltas on storage. Then, the
*persistent* bytes written can be defined as:

```rust
let persistent_bytes_written = positive_delta.saturating_sub(negative_delta);
```

And the "churn" can be defined as the positive deltas that are *not* persistent:

```rust
let churn = positive_delta - persistent_bytes_written;
```

Because tracking these deltas is different from the `SyntheticCost` above, we
also define a `RunningCost` that tracks these, before being converted to the
final 'true' cost by computing the write dimensions. See
[storage-io-cost-modeling.md](storage-io-cost-modeling.md) for the detailed
algorithm for computing `bytes_written` and `bytes_deleted`.

```rust
// Assume addition and integer multiplication is defined on this.
struct RunningCost {
    pub read_time: DurationFP,
    pub compute_time: DurationFP,
    pub bytes_written: u64,
    pub bytes_deleted: u64,
}

impl From<RunningCost> for SyntheticCost {
    fn from(cost: RunningCost) -> SyntheticCost {
        let real_bytes_written = cost.bytes_written.saturating_sub(cost.bytes_deleted);
        SyntheticCost {
            read_time: cost.read_time,
            compute_time: cost.compute_time,
            block_usage: 0,
            bytes_written: real_bytes_written,
            bytes_churned: cost.bytes_written - real_bytes_written,
        }
    }
}
```

### Dimensional Limits & Price Adjustment

The choice of these dimensions is largely because their bounds are
*independent*. No matter how much we save on compute time, if our consensus
cannot broadcast the transaction to all participants in time, it is irrelevant,
and vice-versa. The one wrinkle here is for specifically *synchronous* I/O
reads, which we will count as *both* compute and read time, because they stall
compute. We assume that otherwise, I/O reads and compute are pipelined, so that
compute is never stalled waiting for I/O. Partially this is reasonable because
we know that block validation is frontloaded with compute: Validating all
proofs in a block is compute-heavy, and happens first.

In particular, we have strict limits for four primary dimensions, given by the
time bounds on processing blocks, the amount of data that can be fit into a
block, and the amount of state growth we are willing to tolerate. For the
secondary characteristic, churn, it's not clear what such a limit should be,
but for sanity it makes sense to still set one.

These limits should be part of the ledger's parameters, and be adjustable at
runtime. Note that they are given per block, and nothing is said about
transactions. Initially, we propose these values, which are amenable to
adjustments at any time:

```rust
let block_limits: SyntheticCost {
    read_time: 1 * SECOND,
    compute_time: 1 * SECOND,
    block_usage: 200_000,
    bytes_written: 20_000,
    bytes_churned: 1_000_000,
};
```

A `SyntheticCost` must then be converted into a *fee* denominated in Dust. This
fee is floating, and is dynamically adjusted to target 50% fullness *on each of
the primary dimensions*. Each primary dimension has its own *price* for
fractional block capacity, and this is adjusted upward if blocks are more than
50% full, and downward is they are less than 50% full.

As some dimensions may be consistently less than 50% full other dimensions
dominating demand, we do not wish these to become effectively free. To prevent
this, we impose a lower bound on price adjustment, where any dimension's price
must be at least a fixed fraction of the most expensive dimension's price.

Churn is priced 'the same' as storage cost at a relative level, but with much
higher block capacity, meaning that in relative terms, is a fraction of the
price determined exclusively by the relation of the corresponding block limits.

```rust
struct FeePrices {
    read_time: FixedPoint,
    compute_time: FixedPoint,
    block_usage: FixedPoint,
    bytes_written: FixedPoint,
    // The ratio floor, that the lowest dimension must be at least
    // min_ratio * the highest dimension
    min_ratio: FixedPoint,
}

struct NormalizedCost {
    pub read_time: FixedPoint,
    pub compute_time: FixedPoint,
    pub block_usage: FixedPoint,
    pub bytes_written: FixedPoint,
    pub bytes_churned: FixedPoint,
}

/// Fixed-point rationals. For implementation, a wrapper around `i128` is
// suggested, normalised around `1 << 64` representing 1.0.
type FixedPoint;

impl SyntheticCost {
    fn normalize(self, limits: SyntheticCost) -> Option<NormalizedCost> {
        let res = NormalizedCost {
            read_time: self.read_time / limits.read_time,
            compute_time: self.compute_time / limits.compute_time,
            block_usage: self.block_usage / limits.block_usage,
            bytes_written: self.bytes_written / limits.bytes_written,
            bytes_churned: self.bytes_churned / limits.bytes_churned,
        };
        if res.read_time <= 1 && res.compute_time <= 1 && res.block_usage <= 1 && res.bytes_written <= 1 && res.bytes_churned <= 1 {
            Some(res)
        } else {
            None
        }
    }
}

const INITIAL_PRICES: FeePrices {
    read_time: 10,
    compute_time: 10,
    block_usage: 10,
    bytes_written: 10,
    min_ratio = 0.25;
}

// The curve for taking fullness (0 <= inp <= 1) and returning the factor to
// scale the corresponding fee dimension by. out of scope here.
fn normalized_scaling_curve(inp: FixedPoint) -> FixedPoint;

impl FeePrices {
    fn update(self, block_sum: NormalizedCost) -> Self {
        let mut updated = FeePrices {
            read_time: self.read_time * normalized_scaling_curve(block_sum.read_time),
            compute_time: self.compute_time * normalized_scaling_curve(block_sum.compute_time),
            block_usage: self.block_usage * normalized_scaling_curve(block_sum.block_usage),
            // We use the max between the churn and write dimensions here to be pessimistic.
            bytes_written: self.bytes_written * normalized_scaling_curve(max(block_sum.bytes_written, block_sum.bytes_churned)),
            min_ratio: self.min_ratio,
        };
        let mut most_expensive_dimension = max(updated.read_time, updated.compute_time, updated.block_usage, updated.bytes_written);
        for dimension = [&mut updated.read_time, &mut updated.compute_time, &mut updated.block_usage, &mut updated.bytes_written] {
            // We use a MIN_COST constant here which still allows upwards adjustment. If this was just MIN_POSITIVE, we might get stuck due to rounding.
            *dimension = max(*dimension, most_expensive_dimension * self.min_ratio, FixedPoint::MIN_COST);
        }
        updated
    }
}
```

There is a key difference between the first three dimensions of I/O time,
compute time and block usage, and the two 'storage' dimensions:
The first three are mainly concerned with the *immediate* health of the
consensus protocol, while the latter two are longer term (churn is also quite
short term, but on the order of days, rather than seconds). In particular, they
protect from *short-term denial-of-service* attacks. To be clear, nothing
protects fully here, but the idea is that there is a price to attack the
network, and that an attacker has to pay as much as a regular user.

By contrast, storage is about limiting state growth, and ultimately storage
hardware requirements.

Faced with multiple dimensions, there are two different kinds of
denial-of-service attack that operate very differently:
1. The direct, which wants to simply prevent regular transactions from being
   included. This attack will want to fully saturate the cheapest dimension,
   and will be sustained until price adjustment causes the attacker to run out
   of funds.
2. The out-pricing, which wants to not fully prevent transaction inclusion, but
   to raise prices to a point of unaffordability. This attacker may instead
   target saturating the most expensive dimension, to raise the fees of regular
   transactions more.

In both cases, a denial-of-service attacker has no reason to target more than
one dimension; effectively it is enough for them to 'deny' one of the resource
dimensions, because it is not possible to make transactions without it.

It's not possible to fully combat this, of course, but it *is* possible to make
this attack less effective. We propose doing this by taking the *maximum* price
of the three denial-of-service dimensions, rather than adding these.

This benefits *regular* (resource-balanced) transactions, while being neutral
to denial-of-service transactions that specialize in one dimension.
Storage/write costs and churn are added to this independently.

```rust
impl FeePrices {
    fn overall_cost(self, tx_normalized: NormalizedCost) -> u128 {
        let read_cost = self.read_time * tx_normalized.read_time;
        let compute_cost = self.compute_time * tx_normalized.compute_time;
        let block_cost = self.block_usage * tx_normalized.block_usage;
        let write_cost = self.bytes_written * tx_normalized.bytes_written;
        let churn_cost = self.bytes_written * tx_normalized.bytes_churned;

        let utilization_cost = max(read_cost, compute_cost, block_cost);
        ((utilization_cost + write_cost + churn_cost) * SPECS_PER_DUST).ceil()
    }
}
```

### Synthesising Costs: Basic Operations

To provide a base for our synthetic costs, we carry out benchmarks of low-level
data operations, and measure their single-threaded compute time, as well as
model their I/O interactions.

For I/O interactions, this models the following for basic operations:
- The number of 4k random reads the operation will carry out
- The number of bytes written (pessimistically)
- The number of bytes deleted (assuming the pessimistic write case)

Typically it should be clear from context if the random reads performed are
*batchable* or *synchronous*. (Note that in benchmarking jargon, 'random' reads
typically refers to unbatched reads, which this document refers to as
'synchronous').

I/O writes and churn are therefore modelled directly, with no measurements
involved. I/O read time is modelled partially: The number of batched and
synchronous 4k reads are modelled, and established benchmarks for SSD
performance are applied to convert these into time. Note that we *do not* care
about 'sequential' SSD performance, as our storage and problem domain ensures
random reads dominate.

Initially, to simplify setup, we will benchmark against a specific chosen
baseline machine. In the future, we may expand this to be more representative of
the pool of block-producing nodes.

```rust
struct PerformanceBenchmark {
    pub read_time_batched_4k: Duration,
    pub read_time_synchronous_4k: Duration,
    pub proof_verification_time_constant: Duration,
    pub proof_verification_time_linear: Duration,
    pub verifier_key_load_time: Duration,
    pub transient_hash_time_linear: Duration,
    pub hash_to_curve_time: Duration,
    pub ec_mul_time: Duration,
    pub signature_verification_time: Duration,
    pub fiat_shamir_pedersen_check: Duration,
    // etc ...
}
```

As we skip some of the less costly operations in our modelling of transaction
validation, we add a rough 'baseline' cost for miscellaneous parts of
processing. Further, we mandate a minimum number of cores for validators, which
we include in the cost model as the `parallelism_factor`. These, together with
the full aggregated benchmarks, and the IO characteristics data, form the *cost
model*.

```rust
struct CostModel {
    bench: PerformanceBenchmark,
    parallelism_factor: u16,
    baseline_cost: RunningCost,
    // ...
}
```

For convenience in latter definitions, we assume a number of convenience
functions on `CostModel` that produces the running costs for various high-level
data structure interactions:

```rust
impl CostModel {
    fn proof_verify(&self, public_inputs: u64) -> RunningCost;
    fn verifier_key_load(&self) -> RunningCost;
    fn transient_hash(&self, length: u64) -> RunningCost;
    fn time_filter_map_member(&self) -> RunningCost;
    fn time_filter_map_lookup(&self) -> RunningCost;
    fn time_filter_map_insert(&self, overwrite: bool) -> RunningCost;
    fn cell_read(&self, size: u64) -> RunningCost;
    fn cell_read_sync(&self, size: u64) -> RunningCost;
    fn cell_write(&self, size: u64, overwrite: bool) -> RunningCost;
    fn cell_delete(&self, size: u64) -> RunningCost;
    fn map_member(&self, size: u64) -> RunningCost;
    fn map_index(&self, size: u64) -> RunningCost;
    fn map_insert(&self, size: u64, overwrite: bool) -> RunningCost;
    fn map_remove(&self, size: u64, guaranteed_present: bool) -> RunningCost;
    fn merkle_tree_insert(&self, size: u64, overwrite: bool) -> RunningCost;
    fn merkle_tree_index(&self, size: u64) -> RunningCost;
    // Cost of copying a static (structured) value into the state
    fn tree_copy<T>(&self, value: T) -> RunningCost;
}
```
The implementation of the I/O read costs for these functions is sketched in [storage-io-cost-modeling.md](storage-io-cost-modeling.md).

Though they should not be taken as reference, the parts of the benchmark
highlighted above were mentioned as they are relevant to getting a feeling for
real performance characteristics. For that purpose, we provide some preliminary
values for these here. We also defined a preliminary baseline cost of 100
microseconds of compute, and a parallelism factor of 4.

```rust
// I'm fudging and rounding things here. These should be taken as a rough
// reference, not gospel.
let initial_guideline_bench = PerformanceBenchmark {
    // NOTE: I'm using benchmarks for *sequential* reads for the batched 4k, and
    // *random* reads for the synchronous 4k. This does align with actual
    // performance, because sequential read benchmarks are typically parallel, and "random"
    // ones are synchronous. The performance is *not* about the randomness on
    // SSDs (as it would be on HDDs).
    // Populated using mid-range SSD benchmark results, specifically:
    // https://ssd.userbenchmark.com/SpeedTest/182182/Samsung-SSD-960-PRO-512GB
    // Note that these are MB/s, to get to time per 4k read, it's:
    // x MB/s => x MB/s / 4 kB/block = x/4 k block/s => 0.004/x s / block
    read_time_batched_4k: 2 * MICROSECOND,
    read_time_synchronous_4k: 85 * MICROSECOND,
    // Measured on my own machine
    proof_verification_time_constant: 3_382 * MICROSECOND,
    proof_verification_time_linear: 3_352 * NANOSECOND,
    verifier_key_load_time: 936 * MICROSECOND,
    transient_hash_time_linear: 50 * MICROSECOND,
    hash_to_curve_time: 217 * MICROSECOND,
    ec_mul_time: 90 * MICROSECOND,
    signature_verification_time: 60 * MICROSECOND,
    fiat_shamir_pedersen_check: 180 * MICROSECOND,
    // ...
}

let initial_cost_model = CostModel {
    bench: initial_guideline_bench,
    parallelism_factor: 4,
    baseline_cost: RunningCost {
        read_time: 0,
        compute_time: 100 * MICROSECOND,
        bytes_written: 0,
        bytes_deleted: 0,
    },
    // ...
}
```

### Synthesising Costs: Putting it Together

The costs for processing transactions are then build out of the individual
parts' costs. This is split into validation and application, with validation's
compute time being divided by the multithreading factor to account for its
inherent parallelism.

Generally, this follows the structure of the transaction `well_formed`
function, and state application. Note that it does not need to cover *every* operation
in these, in particular skipping many lightweight consistency checks that are
deemed to be negligible next to those that are included. In addition to this
split, the application stage should be split into the guaranteed segment and the total
cost. This is to allow enforcing separate bounds on the guaranteed segment
execution time.

### Guaranteed Segment limits

The execution time of the guaranteed segment is limited separately and for
different reasons that the overall block limit. Specifically, as nodes need to
execute the guaranteed segment before fees can be taken, this should be
sufficiently low (together with the validation time) to prevent an attacker
from brute-forcing the network with invalid transactions.

This is an imprecise art, as we're comparing very different costs:
- Producing and emitting transactions (from the DoS attacker)
- Processing, verifying, and applying (from the node)

We capture this relation as an input parameter; the *time-to-dismiss*, as a
linear relation of *bytes received*.

```rust
struct LedgerLimits {
    block_limits: SyntheticCost,
    time_to_dismiss_per_byte: Duration,
    // ...
}

struct LedgerParameters {
    limits: LedgerLimits,
    cost_model: CostModel,
    // ...
}

impl Transaction {
    fn dismissable(self, params: LedgerParameters) -> Result<()> {
        let cost_to_dismiss = self.validation_cost(params.cost_model) + self.application_cost(params.cost_model).0;
        let time_to_dismiss = params.limits.time_to_dismiss_per_byte * sizeof(self);
        assert!(cost_to_dismiss.compute_time <= time_to_dismiss);
        assert!(cost_to_dismiss.read_time <= time_to_dismiss);
    }
}
```

`dismissable` should be checked during `well_formed`.

As mentioned, it's not clear what `time_to_dismiss_per_byte` should be set to.
It *is* clear however, that this must be lower-bounded by being apple to verify
proofs that fit within this space. Further, some none-proof transactions are relatively expensive and need to nevertheless be allowed, such as registering Dust generation.

### Tracking writes and deletions 

Due to our storage being deduplicated, tracking insertions and deletions from it
is not as straightfoward as it may appear. This is in particular an issue for
smart contracts, as these are freely programmable, and cannot be analysed by
hand.

Further analysis on how this is dealt with can be found in [this file](./storage-io-cost-modeling.md).
