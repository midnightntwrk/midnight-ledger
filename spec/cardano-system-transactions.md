Cardano-based system transactions on Midnight
=============================================

At launch, Midnight will rely on Cardano as a trusted layer.  This means that
certain information relevant to Midnight will be recorded on Cardano.  Once they
are recorded, they need to be observed on Cardano and persisted in the Midnight
ledger in form of system transactions.  The purpose of this document is to
describe the structure, creation, and verification of Midnight System
Transactions that originate from observing events on Cardano.

Overview
========

Cardano-based Midnight System Transaction (CMST) consist of two components:

  1. **Header**: Contains information needed by Midnight validators to verify
     correctness of the transaction body against the Cardano network.

  2. **Body**: Contains information that originates from Cardano and needs to be
     persisted in the Midnight ledger.

MSTs are constructed by Midnight block producers and verified by Midnight
validators.  Importantly, the header needs to be stored the block history to
enable block verification against Cardano.  However, the ledger itself does not
need the header and can only store the body.

CMST Header
-----------

Header contains information needed to verify correctness of the body against
Cardano.  System transaction body includes all Midnight-relevant events from a
certain period of time.  Time periods covered by subsequent system transactions
must be continuous.  The easiest way of achieving that is the following header:

```rust
struct CMSTHeader {
    block: String,
    tx: u64,
}
```

  * `block`: hash of the last processed Cardano block
  * `index`: index (zero based) of the next transaction to process in the
    `block`.  If `index` equals the size of the block, it means that a block has
    been processed in full.

By inspecting header of the previous system transaction, Midnight block
producers and verifiers can determine the range of Cardano transactions that
should have been included in the payload.

CMST Body
---------

CMSTs are designed to be a universal and extensible way of observing various
events on Cardano.  CMST body is divided into multiple payloads, with each
payload recording different *type* of events that occurred on Cardano.  There is
no requirement that all possible types of payloads exist in a transaction - in
fact, a CMST might contain an empty body.

*Example: If a range of Cardano blocks covered by a system transaction included
events related to DUST production and block rewards, then the body will include
two payloads.  One payload will record information on DUST production, while the
other will record information on block rewards.  Events that occurred on Cardano
are recorded in their dedicated payloads, depending on the type of event.*

When serialized, each payload is tagged with its type, that allows to determine
how it should be decoded.  Serialization format for the CMST should be such,
that it permits future addition of new payload types.

### Payload structure for "cNIGHT generates DUST" scenario

[Proposal 0018: cNIGHT generates
DUST](../proposals/0018-cnight-generates-dust.md) (NgD) describes how DUST can
be generated from NIGHT tokens on Cardano network.  Recall from the
[specification](https://github.com/input-output-hk/midnight-architecture/blob/main/specification/dust.md)
that on Midnight, state of DUST generated from mNIGHT tokens is recorded in DUST
outputs defined as:

```rust
type InitialNonce = Hash<(Hash<Intent>, u32)>;

struct DustOutput {
    value: i128,
    owner: DustPublicKey,
    nonce: field::Hash<(InitialNonce, u32, Fr)>,
    seq: u32,
    ctime: Timestamp,
}
```

where `initial_nonce = hash(night_utxo.intent_hash, night_utxo.output_no)`.  To
compute the balance of the dust output at the time it is spent, DUST generation
information is also needed, and defined as:

```rust
struct DustGenerationInfo {
    value: u128,
    owner: DustPublicKey,
    nonce: InitialNonce,
    dtime: Timestamp,
}
```

An NgD payload stored inside CMST body must contain all the information needed
to create `DustOutput`s and `DustGenerationInfo`s corresponding to NIGHT tokens
on Cardano, just as if they were generated from NIGHT tokens on Midnight.  Let
us first consider what fields need to be generated in a `DustOutput`:

  * `value`: For DUST generated from a newly transferred NIGHT token this is
    assumed to be 0.

  * `owner`: Must be copied over from Cardano.

  * `nonce`: Can be derived from `DustGenerationInfo`, see below.

  * `seq`: Since the DUST will be assigned to a newly transferred NIGHT, this
    will be set to 0.

  * `ctime`: Creation time should be assumed to be the creation time of a
    transaction on Cardano that assigns the right to DUST production.

Now, the fields of `DustGenerationInfo`:

  * `value`: Value of the cNIGHT UTxO.
  * `owner`: Same as the owner in `DustOutput`.
  * `nonce`: Use the Cardano cNIGHT UTxO hash as an arbitrary source of entropy.
  * `dtime`: Copied from Cardano.  See below for detailed discussion.

Summing up, an NgD payload is defined as:

```rust
enum UtxoActionType { Create, Destroy } // ctime or dtime?

struct NgDPayloadEntry {
    value: u128,
    owner: DustPublicKey,
    time: Timestamp,
    action: UtxoActionType,
    nonce: InitialNonce,
}

struct NgDPayload {
    events: Vec<NdGPayloadEntry>
}
```

Building a system transaction
=============================

Building CMST header
--------------------

To build the header of a system transaction, block producer inspects header of
the previous system transaction, as well as the most recent finalized block
produced on Cardano.  This determines the range of Cardano transactions a block
producer should *attempt* to process and include in the body.  Once the building
of transaction body is complete, the header should include the range of
transactions actually processed.

It is possible to process a Cardano block partially:

  * if `index` in the header of previous system transaction equals the number of
    transactions in the `block`, it means that that block has been fully
    processed.  Block producer should start with the next finalized block, if
    such a block exists.

  * if `index` in the header of previous system transaction is strictly smaller
    than the number of transaction in the `block`, it means that the Cardano
    block has been processed partially.  Block producer should begin
    constructing the payload starting with transaction number `index` inside the
    indicated `block`.

*Example: suppose a Cardano block contains 16 transactions.  If the `index` is
16, it means that the whole block has already been processed, and block producer
should begin by producing the next block.  If `index` is 15 it means that block
producer should process the last transaction in the block, and then proceed with
the next block.  If `index` is 0, it means that block producer was aware that a
new block exists but did not process any transaction inside that block.*

Partial processing of a block should only happen if there are too many
transactions, such that including all of them in the payload would exceed CMST
size allowance.  Otherwise, block producer should always process Cardano blocks
fully.

Building CMST body
------------------

To build CMST body, block producer starts processing transactions from the point
indicated by the previous system transaction and identifies any events of
interest.  All events identified are recorded in their respective payloads,
which are then combined to form a body.  We assume that only transactions from
the range identified in the header are processed.  Block producer must ensure
that a payload exists for each type of event that was observed on Cardano and
that each relevant event is recorded in its respective payload.

Building NIGHT generates DUST payload
-------------------------------------

To build NgD payload, block producer looks for transactions of the form
described in "Proposal-0018: cNIGHT generates DUST".  We must be careful to
record the transactions in the same order they were created on Cardano.

**NOTE** A special case of this scenario is after the Glacier Drop, when users
can claim ownership of cNIGHT tokens stored in a vesting contract.  In all
scenarios outlined below such claimed tokens should be treated *as if* they were
stored on a user's wallet, i.e. the user should be able to produce DUST from
these tokens.  The details, such as the datum structure of a versting contract,
are to be determined.

### DUST produced from all cNIGHT tokens in a wallet

Scenario 2 in "cNIGHT generates DUST" proposal assumes a user can register their
Cardano wallet for DUST production.  In this scenario, all cNIGHT tokens
received by a registered wallet must produce DUST to an indicated DUST address
on Midnight.  Block producer must therefore spot and respond to the following
actions occurring on Cardano:

  1. A previously unregistered Cardano wallet is registered for DUST production.

  2. A previously registered Cardano wallet is again registered for DUST
     production, resulting in more than one active registration for the same
     wallet.

  3. A registered Cardano wallet is deregistered, such that there are no more
     active registrations for this wallet.

  4. A new cNIGHT UTxO is sent to a registered wallet address.

  5. A cNIGHT UTxO is spent from a registered wallet address.

Let's walk through each of these actions and discuss how to spot it and how to
respond.

Firstly, before block producer starts processing new transactions, as indicated
by the range determined in the header, they must create a list of already
registered wallets.  This can be done either by inspecting state of the mapping
validator at the beginning of processed Cardano transaction range - see section
"Implementation notes" below, since this can be tricky - or, once created, it
can be continuously updated by inspecting newly created blocks.  From now on we
assume, that for any given Cardano transaction, block producer has a list of
wallets that were registered up until that transaction.

Block producer then proceeds with processing transactions, one by one in order
they appear in the transaction range identified in the header.  Care needs to be
taken to make sure that all events in a transaction are processed.  For example,
a transaction can spend a cNIGHT UTxO from a registered wallet, and sent that
cNIGHT to another registered wallet, requiring two entries in the NgD payload.
Other combinations of actions are also possible and need to be acted on
correctly.

#### Registration of a new wallet

To spot new registrations, block producer checks if a transaction sends new
UTxOs to the mapping validator (c.f "cNIGHT generates DUST" proposal).  Each new
UTxO that contains an authentication token and datum in required format is
considered a new registration.  The block producer should update the list of
currently registered wallets by adding the newly registered wallet to it.

Note, that it is possible that a registration was submitted for an already
registered wallet, as identified by the cNIGHT DUST User identifier field of the
datum (i.e. the `PubKeyHash` of a Cardano wallet).  Block producer must keep
track of this, i.e. record the fact that there are now two registrations.  This
is relevant when processing UTxOs sent to and spent from registered wallet
addresses - see below.

It is also possible that dust address provided in the registration datum is not
valid.  The node should validate dust addresses of submitted registrations.
Only registrations with valid dust addresses are considered valid.
Additionally, presence of a registration with invalid dust address prevents
having a valid registration for the wallet.  In other words, if a wallet submits
one registration with a valid dust address, and another with invalid dust
address, that wallet is considered unregistered.

#### Deregistration of a registered wallet

To spot new deregistrations, block producer checks if a transaction spends UTxOs
from the mapping validator.  Each spent UTxO that contained an authentication
token with a valid datum is considered a deregistration of a wallet indicated in
the datum.  The block producer must remove the registration from the list of
registered wallets.  At this point three options are possible:

  1. A wallet has more than one active registration
  2. A wallet has no more active registrations
  3. A wallet has exactly one active registration

In the first two cases the wallet is considered deregistered.  In the third case
the wallet is considered registered.

#### New cNIGHT UTxOs sent to registered wallet

To spot new cNIGHT UTxOs at a registered address, block producer checks whether
outputs from a transaction contain cNIGHT and, if they do, whether that UTxO is
sent to a registered wallet.  For each new cNIGHT UTxO at a registered wallet
address, block producer creates a payload entry, such that:

  * `value` is the cNIGHT value of the UTxO
  * `owner` is the DUST address from the wallet owner's registration datum
  * `time` is the timestamp of Cardano block that created the UTxO
  * `action` is `Create`
  * `nonce` is the hash of the created cNIGHT UTxO

#### cNIGHT UTxO spent from registered wallet

To spot a cNIGHT UTxOs spent from a registered address, block producer checks
whether inputs to a transaction contain cNIGHT and, if they do, whether that
UTxO is spent from a registered wallet.  For each cNIGHT UTxO spent from a
registered wallet address, block producer creates a payload entry, such that:

  * `value` is the cNIGHT value of the UTxO
  * `owner` is the DUST address from the wallet owner's registration datum
  * `time` is the timestamp of Cardano block that created the UTxO
  * `action` is `Destroy`
  * `nonce` is the hash of the cNIGHT UTxO being spent.

Note: we use cNIGHT UTxO hash as the nonce both for `Create` and `Destroy`
actions, meaning that the nonce will be the same in both cases.

### Leasing access to DUST production

**NOTE:** Support for leasing access to DUST production is not planned to be
implemented before Midnight launch, which means that the contents of this
section is not relevant until Midnight launches.  The description is provided
now, because the design needs to be future proof, i.e. we want Midnight System
Transactions to support the requirements of the leasing scenarios.

Scenarios 3 and 4 in "cNIGHT generates DUST" proposal allow cNIGHT owner to
lease access to DUST production to another user.  In these two scenarios, DUST
is produced from individual cNIGHT UTxOs stored on a leasing validator.  Each
UTxO has a DUST address assigned to it, and a lease end date, after which DUST
production from the UTxO should stop as if it was spent.  Note that the
mentioned two scenarios differ only by who can manage the lease - either the
UTxO owner or an indicated broker - which is irrelevant for the purpose of
building a Midnight system transaction.  Therefore, all UTxO at the leasing
validator are handled in a uniform way.

In order to build NgD payload, block producer must spot and respond to new
leases being created on the leasing validator.  To spot a new lease, block
producer checks transactions for new UTxOs sent to the leasing validator.  Each
new UTxO that contains:

  - a valid datum, as described in "cNIGHT generates DUST" proposal
  - DUST address
  - lease end date set into the future

is considered a new lease.  For each new lease, block producer must create two
entries in the NgD payload.  The first entry marks the start of DUST production
and contains:

  * `value` is the cNIGHT value of the leased UTxO
  * `owner` is the DUST address pointed to in the leased UTxO datum
  * `time` is the timestamp of the block that contains new lease UTxO
  * `action` is `Create`
  * `nonce` is the hash of the UTxO that creates the lease

The second entry marks the end of DUST production and contains:

  * `value` is the cNIGHT value of the leased UTxO
  * `owner` is the DUST address pointed to in the leased UTxO datum
  * `time` is the lease end date from the leased UTxO datum
  * `action` is `Destroy`
  * `nonce` is the hash of the UTxO that creates the lease

Note that:

  * `time` in the second case is set into the future
  * `nonce` in both cases is the same

Empty system transactions and no system transactions
----------------------------------------------------

It might be the case that no Midnight-relevant events occur in a given range of
Cardano blocks.  In such case, block producer should construct MST with an empty
body.  This clearly denotes that a given range of Cardano blocks has already
been processed, which prevents subsequent block producers from inspecting it
unnecessarily.

Block producers should also be allowed to submit blocks that don't contain a
Cardano-based Midnight System Transaction.  This addresses a situation where no
new blocks are produced on Cardano.  In other words, Midnight should be able to
run even if Cardano has not produced a new block since the last Midnight block.
In fact, this will be a common situation since intended transaction throughput
for Midnight is higher than that of Cardano.

Verifying a system transaction
==============================

To verify a Midnight system transaction, validator needs to answer a question:
*"given the transaction header, would I have constructed identical body?"* In
other words, block verifier assumes the range of blocks indicated in the header
of a system transaction, constructs their own body, and checks whether that body
is identical (modulo payload ordering) to the one submitted by the block
producer.  If it is, then the CMST is considered valid and is accepted by the
validator.  If it is different, then the CMST is rejected.

A couple of design notes on this approach:

  1. One could presume that it suffices to check the contents of the body to see
     whether the contained events really happened on Cardano.  However, this
     approach would only enforce soundness, but not completeness.  In other
     words, it would not give us the guarantee that no events were skipped.  By
     requiring the validator to build his own transaction body, we gain the
     guarantee that the block producer did not omit any events.

  2. Approach proposed above assumes that the verifier fully trusts the block
     producer to correctly determine the block range.  This opens the door for a
     dishonest block producer to delay processing of Cardano blocks, by
     submitting fewer blocks than possible.  In particular, per previous
     sub-section, a block producer could submit a block with no CMST even though
     there are unprocessed blocks on Cardano.  Although this is a possibility,
     it should not threaten network security.  We thus accept this risk and
     assume that Midnight verfifier fully trusts the range of blocks specified
     in the header, or blindly accepted the fact that Midnight block does not
     contain a CMST.

Implementation notes
====================

Tracking UTxO state at a specific block
---------------------------------------

Algorithms for building CMST described above assume that we observe state of the
blockchain at the time of a transaction occurring.  For example, when a verifier
node is launch, it must build in memory its own list of currently registered
wallets.  This is achieved by inspecting state of Cardano UTxOs at the point
indicated in the most recent CMST header, which can be in the middle of a
Cardano block.  However, available Cardano indexers might not be able to give us
that information directly.

Therefore, for each DUST-related transaction that requires inspecting the state
of UTxOs, we must compute that exact UTxO state at the moment of transaction
happening.  One approach to do this would be to take the latest state and walk
back from that state, reverting transactions.  Another approach is to write
db-sync queries that filter UTxOs based on block number, but then we need a
special case to handle situations where we have processed a given Cardano block
partially and need to start somewhere from the middle of it.

There is an open question of how to perform all of this efficiently.  Certainly,
we are not interested in all of the blockchain state, but only smart contract
addresses (mapping validator, leasing validator) and registered wallets.  This
could perhaps speed up the process, buy limiting the amount of data.  Whether
this is indeed helpful or not, requires implementing and benchmarking the actual
algorithm.  This is left as an exercise for the reader.

Maintaining a list of registered wallets
----------------------------------------

To correctly track registrations and deregistrations, block producer must
maintain a list of currently registered wallets.  This becomes tricky because
the user may submit multiple registrations for a single wallet.  Such
registrations should be considered invalid for as long as there is more than
one.  To effectively spot the fact that a single registration remains, it is
probably best that a block producer maintains a map from Cardano wallet
`PubKeyHash` to an `Int` that stores number of registrations.  Alternatively, we
might wish to map to a list of UTxOs to identify registering transactions.

Moreover, the node should maintain information on validity of dust addresses of
submitted registrations and make that information available via external API for
the purposes of DUST production DApp.

Timestamps vs block numbers
---------------------------

When building a Midnight System Transaction, block producer operates both on
block hashes/numbers and time stamps.  In particular, there is a need to assign
a concrete timestamp to particular blocks in order to construct the payload.
This can be achieved easily with an indexer such as db-sync, which assigns a UTC
timestamp to every block - see
[here](https://github.com/IntersectMBO/cardano-db-sync/blob/conway-schema-design-13.2/doc/schema.md#block).

Open questions
==============

CMST size limit
---------------

At the moment the maximal permitted size of a CMST inside a Midnight block
remains to be specified.

References
==========

  * [db-sync 13.2 schema
    specification](https://github.com/IntersectMBO/cardano-db-sync/blob/conway-schema-design-13.2/doc/schema.md).
