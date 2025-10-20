use serde_json::json;

#[ignore]
#[tokio::test]
async fn test_contract_initialization_and_rbac() -> Result<(), Box<dyn std::error::Error>> {
    let contract_wasm = near_workspaces::compile_project("./").await?;

    run_integration_flow(&contract_wasm).await?;
    Ok(())
}

async fn run_integration_flow(contract_wasm: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = near_workspaces::sandbox().await?;
    let contract = sandbox.dev_deploy(contract_wasm).await?;

    let admin = sandbox.dev_create_account().await?;
    let executor = sandbox.dev_create_account().await?;
    let beneficiary = sandbox.dev_create_account().await?;
    let token = sandbox.dev_create_account().await?;

    let init_outcome = admin
        .call(contract.id(), "init")
        .args_json(json!({
            "token_id": token.id(),
            "cliff_duration": 1_000u32,
            "full_unlock_duration": 2_000u32,
            "admin": admin.id(),
        }))
        .transact()
        .await?;
    assert!(
        init_outcome.is_success(),
        "{:#?}",
        init_outcome.into_result().unwrap_err()
    );

    let create_grant_outcome = admin
        .call(contract.id(), "create_grant")
        .args_json(json!({
            "account_id": beneficiary.id(),
            "issue_date": 1_000u32,
            "total_amount": "5000",
        }))
        .transact()
        .await?;
    assert!(
        create_grant_outcome.is_success(),
        "{:#?}",
        create_grant_outcome.into_result().unwrap_err()
    );

    let account_view = contract
        .view("get_account")
        .args_json(json!({ "account_id": beneficiary.id() }))
        .await?
        .json::<serde_json::Value>()?;
    assert!(
        !account_view.is_null(),
        "expected account to exist after creating grant"
    );
    assert_eq!(
        account_view["grants"]["1000"]["total_amount"],
        json!("5000")
    );

    let unauthorized_buy = executor
        .call(contract.id(), "buy")
        .args_json(json!({
            "account_ids": [beneficiary.id()],
            "percentage": 5_000u32,
        }))
        .transact()
        .await?;
    assert!(
        unauthorized_buy.is_failure(),
        "expected buy without role to fail"
    );
    let failure = unauthorized_buy.into_result().unwrap_err();
    let failure_message = format!("{failure:?}");
    assert!(
        failure_message.contains("Unauthorized role"),
        "missing RBAC failure, got: {failure_message}"
    );

    Ok(())
}
