# LTIP NEAR Smart Contract – Integration Test Specification

## 1. Shared assumptions & notation

### 1.1 Roles

* **Issuer**
  * Allowed to call: `top_up`, `issue`, `issue_with_transfer` (via `ft_transfer_call`).
* **Executor**
  * Allowed to call: `buy`, `authorize`, `terminate`.
* **Employee**
  * Grantee account (e.g. `alice`, `bob`).
  * Allowed to call: `claim`.

### 1.2 Global config

* `cliff_seconds = 100`
* `vesting_seconds = 900`
* For each grant:
  * `issue_date = T_ISSUE`
  * Cliff ends at: `T_CLIFF_END = issue_date + cliff_seconds`
  * Vesting ends at: `T_VEST_END = T_CLIFF_END + vesting_seconds`
* Example amounts (all in smallest token units, decimals ignored for simplicity):
  * `STANDARD_GRANT_TOTAL = 1_000`

### 1.3 Vested amount function (conceptual)

For a grant with `total_amount` and no termination:
* If `t < T_CLIFF_END`
  `vested(t) = 0`
* If `T_CLIFF_END ≤ t ≤ T_VEST_END`
  `vested(t) = total_amount * (t - T_CLIFF_END) / vesting_seconds`
  (rounded according to contract rules, usually floor)
* If `t > T_VEST_END`
  `vested(t) = total_amount`

If `terminated_at` is set and `terminated_at < T_VEST_END`, then:
* The “effective” vesting end is `terminated_at`.
* `total_amount` is recalculated according to rules below.
* `vested(t)` never exceeds `total_amount`.

### 1.4 Grant and contract state

Each **Grant**:
* `issue_date`
* `total_amount`
* `claimed_amount`
* `order_amount`
* `terminated_at` (optional, `null` if active)

Each **Account (Employee)**:
* Has zero or more `Grant`s.

The **Contract**:
* Holds `spare_balance` — tokens that are not reserved for any grant, can be used to issue new grants or burned.

### 1.5 Termination and spare balance

When a termination changes `total_amount`:
* Let `delta = old_total_amount - new_total_amount`.
* If `delta > 0`, then `spare_balance` **increases** by `delta`.

---

## 2. Test cases

Each test uses the structure:
* **Given** (initial state)
* **When** (action)
* **Then** (expected state & effects)

---

## 2.1 Top up / Issue / Issue with transfer / Claim

### T01 – TopUp increases spare balance

**Description:** Issuer tops up contract; spare balance must increase.

**Given:**

* `spare_balance = 0`
* No grants exist.

**When:**

* At `t = T0`, **Issuer** calls:

  * `top_up(amount = 1_000)`

**Then:**

* `spare_balance = 1_000`
* No `Grant` objects are created or modified.
* No tokens are transferred to any Employee.

---

### T02 – Issue creates grant and reduces spare

**Description:** Issuing a grant reserves tokens from spare for a specific Employee.

**Given:**

* `spare_balance = 1_000`
* Employee `alice` has no grants.

**When:**

* At `t = T_ISSUE`, **Issuer** calls:

  * `issue(account_id = alice, total_amount = 600, issue_date = T_ISSUE)`

**Then:**

* `spare_balance = 1_000 - 600 = 400`
* `alice` has exactly one `Grant G1`:

  * `G1.issue_date = T_ISSUE`
  * `G1.total_amount = 600`
  * `G1.claimed_amount = 0`
  * `G1.order_amount = 0`
  * `G1.terminated_at = null`
* No tokens are transferred out of the contract to `alice` yet.

---

### T03 – IssueWithTransfer uses incoming tokens + spare

**Description:** `issue_with_transfer` (via `ft_transfer_call`) adds tokens to spare and issues a grant in one flow.

**Given:**

* `spare_balance = 200`
* Employee `bob` has no grants.

**When:**

* At `t = T_ISSUE`, **Issuer** calls `ft_transfer_call` to the LTIP contract with:

  * `amount = 500`
  * `msg` instructing contract to `issue_with_transfer`:

    * `account_id = bob`
    * `grant_total = 600`
    * `issue_date = T_ISSUE`

**Then:**

* Contract receives 500 tokens:

  * `spare_balance` temporarily becomes `200 + 500 = 700`
* Contract issues grant `G1` for `bob` with `total_amount = 600`:

  * `spare_balance` after issue = `700 - 600 = 100`
* `bob` has `G1`:

  * `G1.issue_date = T_ISSUE`
  * `G1.total_amount = 600`
  * `G1.claimed_amount = 0`
  * `G1.order_amount = 0`
  * `G1.terminated_at = null`

---

### T04 – Claim creates order for vested minus claimed

**Description:** Employee claims unlocked tokens; this creates an order equal to `vested - claimed`.

**Given:**

* `spare_balance` arbitrary (e.g. 5_000).
* Employee `alice` has grant `G1`:

  * `G1.total_amount = 1_000`
  * `G1.issue_date = T_ISSUE`
  * `G1.claimed_amount = 200`
  * `G1.order_amount = 0`
  * `G1.terminated_at = null`
* Time `T_NOW` is during vesting:

  * `T_CLIFF_END < T_NOW < T_VEST_END`
  * `vested(T_NOW)` for `G1` = 600.

**When:**

* At `t = T_NOW`, **Employee** `alice` calls:

  * `claim(grant_id = G1)`

**Then:**

* `G1.order_amount = 600 - 200 = 400`
* `G1.claimed_amount` remains `200`
* `spare_balance` remains unchanged
* No tokens are transferred on-chain to `alice` in this step.

---

## 2.2 Authorize flow (Employee market release)

Precondition for T05–T07:

* Employee `alice` has `G1`:

  * `G1.total_amount = 1_000`
  * `G1.claimed_amount = 200`
  * `G1.order_amount = 400`
  * `G1.terminated_at = null`
* `spare_balance = 10_000`

### T05 – Authorize 0% (decline)

**Description:** Authorize with percentage `0` means decline; order is cancelled, nothing else changes.

**Given:** Precondition above.

**When:**

* **Executor** calls:

  * `authorize(grant_id = G1, percent = 0)`

**Then:**

* `G1.order_amount = 0`
* `G1.claimed_amount = 200`
* `spare_balance = 10_000`
* No tokens are transferred to `alice`.

---

### T06 – Authorize partial (e.g. 50%)

**Description:** Partially authorize order. Authorized part is transferred to Employee and added to `claimed_amount`. Order is cleared.

**Given:** Precondition above.

**When:**

* **Executor** calls:

  * `authorize(grant_id = G1, percent = 50)`

**Then:**

* Authorized amount = `400 * 50% = 200`
* `G1.order_amount = 0`
* `G1.claimed_amount = 200 + 200 = 400`
* `spare_balance = 10_000` (unchanged)
* Contract transfers `200` tokens to `alice`.

---

### T07 – Authorize 100%

**Description:** Fully authorize order. All ordered tokens go to Employee. Order is cleared.

**Given:** Precondition above.

**When:**

* **Executor** calls:

  * `authorize(grant_id = G1, percent = 100)`

**Then:**

* Authorized amount = 400
* `G1.order_amount = 0`
* `G1.claimed_amount = 200 + 400 = 600`
* `spare_balance = 10_000` (unchanged)
* Contract transfers `400` tokens to `alice`.

---

## 2.3 Buy flow (company buyback)

Precondition for T08–T10:

* `G1.total_amount = 1_000`
* `G1.claimed_amount = 200`
* `G1.order_amount = 400`
* `G1.terminated_at = null`
* `spare_balance = 10_000`

### T08 – Buy 0% (decline)

**Description:** Buy with percent `0` is decline; order cancelled, balances unchanged.

**Given:** Precondition above.

**When:**

* **Executor** calls:

  * `buy(grant_id = G1, percent = 0)`

**Then:**

* `G1.order_amount = 0`
* `G1.claimed_amount = 200`
* `spare_balance = 10_000`
* No transfers involving Employee.

---

### T09 – Buy partial (e.g. 50%)

**Description:** Partially buy order; that part is added to `claimed_amount` and returned to contract `spare_balance`.

**Given:** Precondition above.

**When:**

* **Executor** calls:

  * `buy(grant_id = G1, percent = 50)`

**Then:**

* Bought amount = `400 * 50% = 200`
* `G1.order_amount = 0`
* `G1.claimed_amount = 200 + 200 = 400`
* `spare_balance = 10_000 + 200 = 10_200`
* Employee’s on-chain balance does not change; buyback is represented as tokens returning to spare.

---

### T10 – Buy 100%

**Description:** Fully buy order; whole amount added to `claimed_amount` and to `spare_balance`.

**Given:** Precondition above.

**When:**

* **Executor** calls:

  * `buy(grant_id = G1, percent = 100)`

**Then:**

* Bought amount = 400
* `G1.order_amount = 0`
* `G1.claimed_amount = 200 + 400 = 600`
* `spare_balance = 10_000 + 400 = 10_400`

---

## 2.4 Termination scenarios

Shared assumptions for T11–T19:

* `G1.total_amount = 1_000`
* `G1.issue_date = T_ISSUE`
* `T_CLIFF_END = T_ISSUE + 100`
* `T_VEST_END = T_CLIFF_END + 900`
* `G1.terminated_at = null` initially.
* `spare_balance = SPARE0` initially.

When a termination changes `total_amount`:

* `delta = old_total_amount - new_total_amount`
* If `delta > 0`, then `spare_balance` increases by `delta`.

---

### T11 – Terminate in the future (timestamp between now and vesting end)

**Description:** Termination timestamp is in the future and before vesting end. Total is cut and rest returned to spare.

**Given:**

* Current block time = `T_NOW`
* `T_NOW < T_TERM < T_VEST_END`
* `G1.total_amount = 1_000`
* `G1.claimed_amount = 200`
* `G1.order_amount = 0`
* `G1.terminated_at = null`
* `spare_balance = SPARE0`
* `vested_at_T_TERM = 600`

**When:**

* At `t = T_NOW`, **Executor** calls:

  * `terminate(grant_id = G1, termination_timestamp = T_TERM)`

**Then:**

* `G1.total_amount = 600`
* `G1.terminated_at = T_TERM`
* `G1.claimed_amount = 200`
* `G1.order_amount = 0`
* `spare_balance = SPARE0 + (1_000 - 600) = SPARE0 + 400`

---

### T12 – Terminate now (termination timestamp = current block time during vesting)

**Description:** Same behavior as “future” case but `termination_timestamp == now`.

**Given:**

* `T_CLIFF_END < T_NOW < T_VEST_END`
* `G1.total_amount = 1_000`
* `G1.claimed_amount = 200`
* `G1.order_amount = 0`
* `G1.terminated_at = null`
* `spare_balance = SPARE0`
* `vested_at_T_NOW = 500`

**When:**

* At `t = T_NOW`, **Executor** calls:

  * `terminate(grant_id = G1, termination_timestamp = T_NOW)`

**Then:**

* `G1.total_amount = 500`
* `G1.terminated_at = T_NOW`
* `G1.claimed_amount = 200`
* `G1.order_amount = 0`
* `spare_balance = SPARE0 + (1_000 - 500) = SPARE0 + 500`

---

### T13 – Terminate in the past, claimed ≤ vested(timestamp)

**Description:** Timestamp is in the past and `claimed_amount ≤ vested(termination_timestamp)`. Total is recalculated to vested amount; order trimmed only if needed.

**Given:**

* Current time = `T_NOW`
* `T_ISSUE < T_TERM < T_NOW` and `T_CLIFF_END < T_TERM < T_VEST_END`
* `G1.total_amount = 1_000`
* `G1.claimed_amount = 200`
* `G1.order_amount = 300`
* `G1.terminated_at = null`
* `spare_balance = SPARE0`
* `vested_at_T_TERM = 600`
* `claimed_amount (200) ≤ vested_at_T_TERM (600)`

**When:**

* At `t = T_NOW`, **Executor** calls:

  * `terminate(grant_id = G1, termination_timestamp = T_TERM)`

**Then:**

* `G1.total_amount = 600`
* `claimed + order = 200 + 300 = 500 ≤ 600` → `order_amount` remains 300
* `G1.claimed_amount = 200`
* `G1.order_amount = 300`
* `G1.terminated_at = T_TERM`
* `spare_balance = SPARE0 + (1_000 - 600) = SPARE0 + 400`

---

### T14 – Terminate in the past, order would exceed new total

**Description:** Same branch as T13, but `claimed + order > new_total`, so order must be trimmed.

**Given:**

* Same as T13, except:

  * `G1.claimed_amount = 500`
  * `G1.order_amount = 200`
  * `vested_at_T_TERM = 600`
  * `claimed + order = 700 > 600`

**When:**

* **Executor** calls:

  * `terminate(grant_id = G1, termination_timestamp = T_TERM)`

**Then:**

* `G1.total_amount = 600`
* `G1.claimed_amount = 500`
* `G1.order_amount` is reduced so that `claimed + order ≤ total`:

  * `G1.order_amount = 600 - 500 = 100`
* `G1.terminated_at = T_TERM`
* `spare_balance = SPARE0 + (1_000 - 600) = SPARE0 + 400`

---

### T15 – Terminate in the past, claimed > vested(timestamp)

**Description:** Timestamp is in the past but `claimed_amount > vested(termination_timestamp)`. Termination is moved to last claim date, total becomes claimed, order cleared.

**Given:**

* Current time = `T_NOW`
* `T_TERM < T_CLAIM < T_NOW`
* At `T_CLAIM`, Employee last claimed
* `G1.total_amount = 1_000`
* `G1.claimed_amount = 400`
* `G1.order_amount = 150`
* `G1.terminated_at = null`
* `spare_balance = SPARE0`
* `vested_at_T_TERM = 300`
* `claimed (400) > vested_at_T_TERM (300)`

**When:**

* **Executor** calls:

  * `terminate(grant_id = G1, termination_timestamp = T_TERM)`

**Then:**

* Effective termination timestamp is moved forward to `T_CLAIM`
* `G1.total_amount = G1.claimed_amount = 400`
* `G1.order_amount = 0`
* `G1.terminated_at = T_CLAIM`
* `spare_balance = SPARE0 + (1_000 - 400) = SPARE0 + 600`

> Matches: **“If claimed > vested -> total = claimed, termination timestamp set to claim date.”**

---

### T16 – Terminate before issue_date

**Description:** Termination timestamp strictly before `issue_date`. Entire grant is effectively cancelled; all tokens go back to spare.

**Given:**

* `T_TERM < T_ISSUE`
* `G1.total_amount = 1_000`
* `G1.claimed_amount = 0`
* `G1.order_amount = 0`
* `G1.terminated_at = null`
* `spare_balance = SPARE0`

**When:**

* **Executor** calls:

  * `terminate(grant_id = G1, termination_timestamp = T_TERM)`

**Then:**

* No vesting possible before issue:

  * `G1.total_amount = 0`
* `G1.claimed_amount = 0`
* `G1.order_amount = 0`
* `G1.terminated_at = T_TERM`
* `spare_balance = SPARE0 + 1_000`

> Scenario: **“Terminate: before issue”**

---

### T17 – Terminate before cliff (after issue, before cliff end)

**Description:** Termination timestamp is after issue, but before cliff end. Nothing vested yet; entire grant goes back to spare.

**Given:**

* `T_ISSUE < T_TERM < T_CLIFF_END`
* `G1.total_amount = 1_000`
* `G1.claimed_amount = 0`
* `G1.order_amount = 0`
* `G1.terminated_at = null`
* `spare_balance = SPARE0`

**When:**

* **Executor** calls:

  * `terminate(grant_id = G1, termination_timestamp = T_TERM)`

**Then:**

* `vested_at_T_TERM = 0`
* `G1.total_amount = 0`
* `G1.claimed_amount = 0`
* `G1.order_amount = 0`
* `G1.terminated_at = T_TERM`
* `spare_balance = SPARE0 + 1_000`

> Scenario: **“Terminate: before cliff”**

---

### T18 – Terminate during vesting, no orders

**Description:** Termination during vesting period, with no outstanding orders.

**Given:**

* `T_CLIFF_END < T_TERM < T_VEST_END`
* `G1.total_amount = 1_000`
* `G1.claimed_amount = 200`
* `G1.order_amount = 0`
* `G1.terminated_at = null`
* `spare_balance = SPARE0`
* `vested_at_T_TERM = 550`

**When:**

* **Executor** calls:

  * `terminate(grant_id = G1, termination_timestamp = T_TERM)`

**Then:**

* `G1.total_amount = 550`
* `G1.claimed_amount = 200`
* `G1.order_amount = 0`
* `G1.terminated_at = T_TERM`
* `spare_balance = SPARE0 + (1_000 - 550) = SPARE0 + 450`

> Scenario: **“Terminate: during vesting, when no orders”**

---

### T19 – Terminate during vesting, with orders

**Description:** Termination during vesting, with outstanding order. Order may need trimming.

**Given:**

* `T_CLIFF_END < T_TERM < T_VEST_END`
* `G1.total_amount = 1_000`
* `G1.claimed_amount = 200`
* `G1.order_amount = 400`
* `G1.terminated_at = null`
* `spare_balance = SPARE0`
* `vested_at_T_TERM = 500`
* `claimed + order = 200 + 400 = 600 > 500`

**When:**

* **Executor** calls:

  * `terminate(grant_id = G1, termination_timestamp = T_TERM)`

**Then:**

* `G1.total_amount = 500`
* `G1.claimed_amount = 200`
* `G1.order_amount` reduced so that `claimed + order ≤ total`:

  * `G1.order_amount = 500 - 200 = 300`
* `G1.terminated_at = T_TERM`
* `spare_balance = SPARE0 + (1_000 - 500) = SPARE0 + 500`

> Scenario: **“Terminate: during vesting, when has orders”**
