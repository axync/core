use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use std::collections::HashMap;
use axync_types::{DealVisibility, TxKind, TxPayload};
use std::sync::Arc;
use axync_sequencer::Sequencer;
use axync_storage::Storage;
use axync_types::{AssetId, BlockId, DealId};

use crate::types::*;
use axync_sequencer::security::{sanitize_string, validate_hex_string};

pub struct ApiState {
    pub sequencer: Arc<Sequencer>,
    pub storage: Option<Arc<dyn Storage>>,
    pub rate_limit_state: Option<Arc<crate::middleware::RateLimitState>>,
    pub vesting_reader: Option<Arc<crate::vesting::VestingReader>>,
    pub escrow_reader: Option<Arc<crate::escrow::EscrowReader>>,
    pub nft_reader: Option<Arc<crate::nft::NftReader>>,
    pub sablier_contracts: Vec<String>,
    pub hedgey_contracts: Vec<String>,
}

pub async fn get_account_balance(
    State(state): State<Arc<ApiState>>,
    Path((address, asset_id)): Path<(String, AssetId)>,
) -> Result<Json<AccountBalanceResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Sanitize and validate input
    let sanitized_address = sanitize_string(&address);
    
    if !validate_hex_string(&sanitized_address) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "InvalidAddress".to_string(),
                message: "Invalid address format".to_string(),
            }),
        ));
    }
    
    let address_bytes = hex::decode(sanitized_address.trim_start_matches("0x")).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "InvalidAddress".to_string(),
                message: "Invalid address format".to_string(),
            }),
        )
    })?;

    if address_bytes.len() != 20 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "InvalidAddress".to_string(),
                message: "Address must be 20 bytes".to_string(),
            }),
        ));
    }

    let mut addr = [0u8; 20];
    addr.copy_from_slice(&address_bytes);

    let state_handle = state.sequencer.get_state();
    let state_guard = state_handle.lock().unwrap();

    let account = state_guard.get_account_by_address(addr).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "AccountNotFound".to_string(),
                message: "Account not found".to_string(),
            }),
        )
    })?;

    let balances: Vec<crate::types::AssetBalanceEntry> = account
        .balances
        .iter()
        .filter(|b| b.asset_id == asset_id)
        .map(|b| crate::types::AssetBalanceEntry {
            chain_id: b.chain_id,
            amount: b.amount,
        })
        .collect();

    Ok(Json(AccountBalanceResponse {
        address: addr,
        asset_id,
        balances,
    }))
}

pub async fn get_account_state(
    State(state): State<Arc<ApiState>>,
    Path(address): Path<String>,
) -> Result<Json<AccountStateResponse>, (StatusCode, Json<ErrorResponse>)> {
    let address_bytes = hex::decode(address.trim_start_matches("0x")).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "InvalidAddress".to_string(),
                message: "Invalid address format".to_string(),
            }),
        )
    })?;

    if address_bytes.len() != 20 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "InvalidAddress".to_string(),
                message: "Address must be 20 bytes".to_string(),
            }),
        ));
    }

    let mut addr = [0u8; 20];
    addr.copy_from_slice(&address_bytes);

    let state_handle = state.sequencer.get_state();
    let mut state_guard = state_handle.lock().unwrap();

    // Create account automatically if it doesn't exist (on first login/request)
    // This matches the behavior of get_or_create_account_by_owner used in transactions
    let account = state_guard.get_or_create_account_by_owner(addr);
    
    // Extract account data before releasing the mutable borrow
    let account_id = account.id;
    let nonce = account.nonce;
    let balances: Vec<BalanceInfo> = account
        .balances
        .iter()
        .map(|b| BalanceInfo {
            asset_id: b.asset_id,
            chain_id: b.chain_id,
            amount: b.amount,
        })
        .collect();
    
    // Now we can use immutable borrow for deals
    let open_deals: Vec<DealId> = state_guard
        .deals
        .values()
        .filter(|deal| {
            (deal.maker == addr || deal.taker == Some(addr))
                && matches!(deal.status, axync_types::DealStatus::Pending)
        })
        .map(|deal| deal.id)
        .collect();

    Ok(Json(AccountStateResponse {
        address: addr,
        account_id,
        balances,
        nonce,
        open_deals,
    }))
}

pub async fn get_deals_list(
    State(state): State<Arc<ApiState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<DealListResponse>, (StatusCode, Json<ErrorResponse>)> {
    let state_handle = state.sequencer.get_state();
    let state_guard = state_handle.lock().unwrap();

    let mut deals: Vec<DealDetailsResponse> = state_guard
        .deals
        .values()
        .map(|deal| DealDetailsResponse {
            deal_id: deal.id,
            maker: deal.maker,
            taker: deal.taker,
            offer: crate::types::TradeAssetJson::from_trade_asset(&deal.offer),
            consideration: crate::types::TradeAssetJson::from_trade_asset(&deal.consideration),
            amount_filled: deal.amount_filled,
            status: format!("{:?}", deal.status),
            created_at: deal.created_at,
            expires_at: deal.expires_at,
            is_cross_chain: deal.is_cross_chain,
        })
        .collect();

    // Filter by status if provided
    if let Some(status_filter) = params.get("status") {
        let status_str = status_filter.to_lowercase();
        deals.retain(|deal| deal.status.to_lowercase() == status_str);
    }

    // Filter by address (maker or taker) if provided
    if let Some(address_filter) = params.get("address") {
        let address_bytes = hex::decode(address_filter.trim_start_matches("0x")).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "InvalidAddress".to_string(),
                    message: "Invalid address format".to_string(),
                }),
            )
        })?;

        if address_bytes.len() != 20 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "InvalidAddress".to_string(),
                    message: "Address must be 20 bytes".to_string(),
                }),
            ));
        }

        let mut addr = [0u8; 20];
        addr.copy_from_slice(&address_bytes);

        deals.retain(|deal| deal.maker == addr || deal.taker == Some(addr));
    }

    // Filter by visibility if provided
    if let Some(visibility_filter) = params.get("visibility") {
        let _visibility_str = visibility_filter.to_lowercase();
        // Note: visibility is not in DealDetailsResponse, so we need to check the original deal
        // For now, we'll skip this filter or add visibility to the response
        // This is a limitation we can address later if needed
    }

    let total = deals.len();

    Ok(Json(DealListResponse { deals, total }))
}

pub async fn get_deal_details(
    State(state): State<Arc<ApiState>>,
    Path(deal_id): Path<DealId>,
) -> Result<Json<DealDetailsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let state_handle = state.sequencer.get_state();
    let state_guard = state_handle.lock().unwrap();

    let deal = state_guard.get_deal(deal_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "DealNotFound".to_string(),
                message: format!("Deal {} not found", deal_id),
            }),
        )
    })?;

    Ok(Json(DealDetailsResponse {
        deal_id: deal.id,
        maker: deal.maker,
        taker: deal.taker,
        offer: crate::types::TradeAssetJson::from_trade_asset(&deal.offer),
        consideration: crate::types::TradeAssetJson::from_trade_asset(&deal.consideration),
        amount_filled: deal.amount_filled,
        status: format!("{:?}", deal.status),
        created_at: deal.created_at,
        expires_at: deal.expires_at,
        is_cross_chain: deal.is_cross_chain,
    }))
}

pub async fn get_block_info(
    State(state): State<Arc<ApiState>>,
    Path(block_id): Path<BlockId>,
) -> Result<Json<BlockInfoResponse>, (StatusCode, Json<ErrorResponse>)> {
    let block = if let Some(ref storage) = state.storage {
        storage
            .get_block(block_id)
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "StorageError".to_string(),
                        message: "Failed to load block from storage".to_string(),
                    }),
                )
            })?
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: "BlockNotFound".to_string(),
                        message: format!("Block {} not found", block_id),
                    }),
                )
            })?
    } else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "StorageNotAvailable".to_string(),
                message: "Storage not configured".to_string(),
            }),
        ));
    };

    let transactions: Vec<TransactionInfo> = block
        .transactions
        .iter()
        .map(|tx| TransactionInfo {
            id: tx.id,
            from: tx.from,
            nonce: tx.nonce,
            kind: format!("{:?}", tx.kind),
        })
        .collect();

    Ok(Json(BlockInfoResponse {
        block_id: block.id,
        transaction_count: block.transactions.len(),
        timestamp: block.timestamp,
        state_root: format!("0x{}", hex::encode(block.state_root)),
        withdrawals_root: format!("0x{}", hex::encode(block.withdrawals_root)),
        block_proof: format!("0x{}", hex::encode(&block.block_proof)),
        transactions,
    }))
}

pub async fn get_current_block_id(State(state): State<Arc<ApiState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "current_block_id": state.sequencer.get_current_block_id()
    }))
}

pub async fn get_queue_status(State(state): State<Arc<ApiState>>) -> Json<QueueStatusResponse> {
    Json(QueueStatusResponse {
        pending_transactions: state.sequencer.queue_length(),
        max_queue_size: 10000,
        current_block_id: state.sequencer.get_current_block_id(),
    })
}

pub async fn get_supported_chains() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "chains": [
            {
                "chain_id": axync_types::chain_ids::ETHEREUM,
                "name": "Ethereum"
            },
            {
                "chain_id": axync_types::chain_ids::POLYGON,
                "name": "Polygon"
            },
            {
                "chain_id": axync_types::chain_ids::BASE,
                "name": "Base"
            },
            {
                "chain_id": axync_types::chain_ids::ARBITRUM,
                "name": "Arbitrum"
            },
            {
                "chain_id": axync_types::chain_ids::OPTIMISM,
                "name": "Optimism"
            },
            {
                "chain_id": axync_types::chain_ids::ETHEREUM_SEPOLIA,
                "name": "Ethereum Sepolia"
            },
            {
                "chain_id": axync_types::chain_ids::BASE_SEPOLIA,
                "name": "Base Sepolia"
            }
        ]
    }))
}

pub async fn jsonrpc_handler(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    if request.jsonrpc != "2.0" {
        return Json(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: "Invalid Request".to_string(),
                data: None,
            }),
            id: request.id,
        });
    }

    let result = match request.method.as_str() {
        "submit_tx" => {
            let tx_hex = match request.params.get("tx") {
                Some(serde_json::Value::String(hex_str)) => hex_str,
                _ => {
                    return Json(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: "Invalid params: 'tx' must be a hex string".to_string(),
                            data: None,
                        }),
                        id: request.id,
                    });
                }
            };

            let tx_bytes = match hex::decode(tx_hex.trim_start_matches("0x")) {
                Ok(bytes) => bytes,
                Err(_) => {
                    return Json(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: "Invalid params: 'tx' must be valid hex".to_string(),
                            data: None,
                        }),
                        id: request.id,
                    });
                }
            };

            let tx: axync_types::Tx = match bincode::deserialize(&tx_bytes) {
                Ok(tx) => tx,
                Err(_) => {
                    return Json(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32602,
                            message: "Invalid params: failed to deserialize transaction"
                                .to_string(),
                            data: None,
                        }),
                        id: request.id,
                    });
                }
            };

            match state.sequencer.submit_tx(tx) {
                Ok(()) => {
                    let tx_hash = hex::encode(&tx_bytes);
                    Some(serde_json::json!({
                        "tx_hash": tx_hash,
                        "status": "queued"
                    }))
                }
                Err(axync_sequencer::SequencerError::QueueFull) => {
                    return Json(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32000,
                            message: "Queue full".to_string(),
                            data: None,
                        }),
                        id: request.id,
                    });
                }
                Err(axync_sequencer::SequencerError::InvalidSignature) => {
                    return Json(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32001,
                            message: "Invalid signature".to_string(),
                            data: None,
                        }),
                        id: request.id,
                    });
                }
                Err(axync_sequencer::SequencerError::InvalidNonce) => {
                    return Json(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32002,
                            message: "Invalid nonce".to_string(),
                            data: None,
                        }),
                        id: request.id,
                    });
                }
                Err(e) => {
                    return Json(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32003,
                            message: format!("Submission failed: {:?}", e),
                            data: None,
                        }),
                        id: request.id,
                    });
                }
            }
        }
        "get_account_balance" => {
            return Json(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: "Use REST endpoint /api/v1/account/:address/balance/:asset_id instead"
                        .to_string(),
                    data: None,
                }),
                id: request.id,
            });
        }
        _ => {
            return Json(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: "Method not found".to_string(),
                    data: None,
                }),
                id: request.id,
            });
        }
    };

    Json(JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        result,
        error: None,
        id: request.id,
    })
}

pub async fn submit_transaction(
    State(state): State<Arc<ApiState>>,
    Json(request): Json<crate::types::SubmitTransactionRequest>,
) -> Result<Json<crate::types::SubmitTransactionResponse>, (StatusCode, Json<ErrorResponse>)> {
    use crate::types::SubmitTransactionRequest;
    use axync_types::Tx;

    let (tx, _from_address) = match request {
        SubmitTransactionRequest::Deposit {
            tx_hash,
            account,
            asset_id,
            amount,
            chain_id,
            nonce,
            signature,
        } => {
            let tx_hash_bytes = hex::decode(tx_hash.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidTxHash".to_string(),
                            message: "Invalid tx_hash format".to_string(),
                        }),
                    )
                })?;

            if tx_hash_bytes.len() != 32 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidTxHash".to_string(),
                        message: "tx_hash must be 32 bytes".to_string(),
                    }),
                ));
            }

            let mut tx_hash_array = [0u8; 32];
            tx_hash_array.copy_from_slice(&tx_hash_bytes);

            let account_bytes = hex::decode(account.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidAddress".to_string(),
                            message: "Invalid account address format".to_string(),
                        }),
                    )
                })?;

            if account_bytes.len() != 20 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidAddress".to_string(),
                        message: "Account address must be 20 bytes".to_string(),
                    }),
                ));
            }

            let mut addr = [0u8; 20];
            addr.copy_from_slice(&account_bytes);

            let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidSignature".to_string(),
                            message: "Invalid signature format".to_string(),
                        }),
                    )
                })?;

            if sig_bytes.len() != 65 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidSignature".to_string(),
                        message: "Signature must be 65 bytes".to_string(),
                    }),
                ));
            }

            let mut sig = [0u8; 65];
            sig.copy_from_slice(&sig_bytes);

            let tx = Tx {
                id: 0,
                from: addr,
                nonce,
                kind: TxKind::Deposit,
                payload: TxPayload::Deposit(axync_types::Deposit {
                    tx_hash: tx_hash_array,
                    account: addr,
                    asset_id,
                    amount,
                    chain_id,
                }),
                signature: sig,
            };

            (tx, addr)
        }
        SubmitTransactionRequest::CreateDeal {
            from,
            deal_id,
            visibility,
            taker,
            offer,
            consideration,
            expires_at,
            external_ref,
            nonce,
            signature,
        } => {
            let from_bytes = hex::decode(from.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidAddress".to_string(),
                            message: "Invalid from address format".to_string(),
                        }),
                    )
                })?;

            if from_bytes.len() != 20 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidAddress".to_string(),
                        message: "From address must be 20 bytes".to_string(),
                    }),
                ));
            }

            let mut from_address = [0u8; 20];
            from_address.copy_from_slice(&from_bytes);

            let visibility_enum = match visibility.as_str() {
                "Public" => DealVisibility::Public,
                "Direct" => DealVisibility::Direct,
                _ => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidVisibility".to_string(),
                            message: "Visibility must be 'Public' or 'Direct'".to_string(),
                        }),
                    ));
                }
            };

            let taker_addr = taker.and_then(|t| {
                let bytes = hex::decode(t.trim_start_matches("0x")).ok()?;
                if bytes.len() != 20 {
                    return None;
                }
                let mut addr = [0u8; 20];
                addr.copy_from_slice(&bytes);
                Some(addr)
            });

            let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidSignature".to_string(),
                            message: "Invalid signature format".to_string(),
                        }),
                    )
                })?;

            if sig_bytes.len() != 65 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidSignature".to_string(),
                        message: "Signature must be 65 bytes".to_string(),
                    }),
                ));
            }

            let mut sig = [0u8; 65];
            sig.copy_from_slice(&sig_bytes);

            let tx = Tx {
                id: 0,
                from: from_address,
                nonce,
                kind: TxKind::CreateDeal,
                payload: TxPayload::CreateDeal(axync_types::CreateDeal {
                    deal_id,
                    visibility: visibility_enum,
                    taker: taker_addr,
                    offer: offer.to_trade_asset(),
                    consideration: consideration.to_trade_asset(),
                    expires_at,
                    external_ref,
                }),
                signature: sig,
            };

            (tx, from_address)
        }
        SubmitTransactionRequest::AcceptDeal {
            from,
            deal_id,
            amount,
            nonce,
            signature,
        } => {
            let from_bytes = hex::decode(from.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidAddress".to_string(),
                            message: "Invalid from address format".to_string(),
                        }),
                    )
                })?;

            if from_bytes.len() != 20 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidAddress".to_string(),
                        message: "From address must be 20 bytes".to_string(),
                    }),
                ));
            }

            let mut from_address = [0u8; 20];
            from_address.copy_from_slice(&from_bytes);

            let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidSignature".to_string(),
                            message: "Invalid signature format".to_string(),
                        }),
                    )
                })?;

            if sig_bytes.len() != 65 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidSignature".to_string(),
                        message: "Signature must be 65 bytes".to_string(),
                    }),
                ));
            }

            let mut sig = [0u8; 65];
            sig.copy_from_slice(&sig_bytes);

            let tx = Tx {
                id: 0,
                from: from_address,
                nonce,
                kind: TxKind::AcceptDeal,
                payload: TxPayload::AcceptDeal(axync_types::AcceptDeal {
                    deal_id,
                    amount,
                }),
                signature: sig,
            };

            (tx, from_address)
        }
        SubmitTransactionRequest::CancelDeal {
            from,
            deal_id,
            nonce,
            signature,
        } => {
            let from_bytes = hex::decode(from.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidAddress".to_string(),
                            message: "Invalid from address format".to_string(),
                        }),
                    )
                })?;

            if from_bytes.len() != 20 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidAddress".to_string(),
                        message: "From address must be 20 bytes".to_string(),
                    }),
                ));
            }

            let mut from_address = [0u8; 20];
            from_address.copy_from_slice(&from_bytes);

            let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidSignature".to_string(),
                            message: "Invalid signature format".to_string(),
                        }),
                    )
                })?;

            if sig_bytes.len() != 65 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidSignature".to_string(),
                        message: "Signature must be 65 bytes".to_string(),
                    }),
                ));
            }

            let mut sig = [0u8; 65];
            sig.copy_from_slice(&sig_bytes);

            let tx = Tx {
                id: 0,
                from: from_address,
                nonce,
                kind: TxKind::CancelDeal,
                payload: TxPayload::CancelDeal(axync_types::CancelDeal { deal_id }),
                signature: sig,
            };

            (tx, from_address)
        }
        SubmitTransactionRequest::Withdraw {
            from,
            asset_id,
            amount,
            to,
            chain_id,
            nonce,
            signature,
        } => {
            let from_bytes = hex::decode(from.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidAddress".to_string(),
                            message: "Invalid from address format".to_string(),
                        }),
                    )
                })?;

            if from_bytes.len() != 20 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidAddress".to_string(),
                        message: "From address must be 20 bytes".to_string(),
                    }),
                ));
            }

            let mut from_address = [0u8; 20];
            from_address.copy_from_slice(&from_bytes);

            let to_bytes = hex::decode(to.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidAddress".to_string(),
                            message: "Invalid to address format".to_string(),
                        }),
                    )
                })?;

            if to_bytes.len() != 20 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidAddress".to_string(),
                        message: "To address must be 20 bytes".to_string(),
                    }),
                ));
            }

            let mut to_address = [0u8; 20];
            to_address.copy_from_slice(&to_bytes);

            let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidSignature".to_string(),
                            message: "Invalid signature format".to_string(),
                        }),
                    )
                })?;

            if sig_bytes.len() != 65 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidSignature".to_string(),
                        message: "Signature must be 65 bytes".to_string(),
                    }),
                ));
            }

            let mut sig = [0u8; 65];
            sig.copy_from_slice(&sig_bytes);

            let tx = Tx {
                id: 0,
                from: from_address,
                nonce,
                kind: TxKind::Withdraw,
                payload: TxPayload::Withdraw(axync_types::Withdraw {
                    asset_id,
                    amount,
                    to: to_address,
                    chain_id,
                }),
                signature: sig,
            };

            (tx, from_address)
        }
        SubmitTransactionRequest::BuyNft {
            from,
            listing_id,
            nonce,
            signature,
        } => {
            let from_bytes = hex::decode(from.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidAddress".to_string(),
                            message: "Invalid from address format".to_string(),
                        }),
                    )
                })?;

            if from_bytes.len() != 20 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidAddress".to_string(),
                        message: "From address must be 20 bytes".to_string(),
                    }),
                ));
            }

            let mut from_address = [0u8; 20];
            from_address.copy_from_slice(&from_bytes);

            let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "InvalidSignature".to_string(),
                            message: "Invalid signature format".to_string(),
                        }),
                    )
                })?;

            if sig_bytes.len() != 65 {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "InvalidSignature".to_string(),
                        message: "Signature must be 65 bytes".to_string(),
                    }),
                ));
            }

            let mut sig = [0u8; 65];
            sig.copy_from_slice(&sig_bytes);

            let tx = Tx {
                id: 0,
                from: from_address,
                nonce,
                kind: TxKind::BuyNft,
                payload: TxPayload::BuyNft(axync_types::BuyNft {
                    listing_id,
                }),
                signature: sig,
            };

            (tx, from_address)
        }
    };

    // Serialize transaction before submitting (for tx_hash generation)
    let tx_hash = hex::encode(&bincode::serialize(&tx).unwrap_or_default());
    
    match state.sequencer.submit_tx_with_validation(tx, true) {
        Ok(()) => {
            Ok(Json(crate::types::SubmitTransactionResponse {
                tx_hash,
                status: "queued".to_string(),
            }))
        }
        Err(axync_sequencer::SequencerError::QueueFull) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "QueueFull".to_string(),
                message: "Transaction queue is full".to_string(),
            }),
        )),
        Err(axync_sequencer::SequencerError::InvalidSignature) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "InvalidSignature".to_string(),
                message: "Transaction signature is invalid".to_string(),
            }),
        )),
        Err(axync_sequencer::SequencerError::InvalidNonce) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "InvalidNonce".to_string(),
                message: "Transaction nonce is invalid".to_string(),
            }),
        )),
        Err(axync_sequencer::SequencerError::ExecutionFailed(stf_err)) => {
            // Extract error message from StfError
            let error_msg = format!("{:?}", stf_err);
            let (error_code, message): (String, String) = if error_msg.contains("BalanceTooLow") {
                (
                    "BalanceTooLow".to_string(),
                    "Insufficient balance to execute this transaction. Please ensure you have enough funds on the required chain.".to_string(),
                )
            } else if error_msg.contains("DealNotFound") {
                (
                    "DealNotFound".to_string(),
                    "The deal you are trying to accept does not exist.".to_string(),
                )
            } else if error_msg.contains("DealAlreadyClosed") {
                (
                    "DealAlreadyClosed".to_string(),
                    "This deal has already been settled or cancelled.".to_string(),
                )
            } else if error_msg.contains("Unauthorized") {
                (
                    "Unauthorized".to_string(),
                    "You are not authorized to perform this action on this deal.".to_string(),
                )
            } else if error_msg.contains("InvalidNonce") {
                (
                    "InvalidNonce".to_string(),
                    "Transaction nonce is invalid. Please try again.".to_string(),
                )
            } else if error_msg.contains("DealExpired") {
                (
                    "DealExpired".to_string(),
                    "This deal has expired.".to_string(),
                )
            } else {
                (
                    "ExecutionFailed".to_string(),
                    format!("Transaction execution failed: {}", error_msg),
                )
            };
            
            Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: error_code,
                    message: message,
                }),
            ))
        },
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "SubmissionFailed".to_string(),
                message: format!("Failed to submit transaction: {:?}", e),
            }),
        )),
    }
}

// ══════════════════════════════════════════════
// ██  VESTING MARKETPLACE ENDPOINTS
// ══════════════════════════════════════════════

pub async fn get_vesting_positions(
    State(state): State<Arc<ApiState>>,
    Path(address): Path<String>,
) -> Result<Json<VestingPositionsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let reader = state.vesting_reader.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "VestingNotConfigured".to_string(),
                message: "Vesting reader not configured".to_string(),
            }),
        )
    })?;

    let clean_addr = if address.starts_with("0x") {
        address.clone()
    } else {
        format!("0x{}", address)
    };

    let sablier_refs: Vec<&str> = state.sablier_contracts.iter().map(|s| s.as_str()).collect();
    let hedgey_refs: Vec<&str> = state.hedgey_contracts.iter().map(|s| s.as_str()).collect();

    let positions = reader.get_all_positions(&clean_addr, &sablier_refs, &hedgey_refs).await;
    let total = positions.len();

    Ok(Json(VestingPositionsResponse {
        address: clean_addr,
        positions,
        total,
    }))
}

pub async fn get_listings(
    State(state): State<Arc<ApiState>>,
) -> Result<Json<ListingsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let reader = state.escrow_reader.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "EscrowNotConfigured".to_string(),
                message: "Escrow reader not configured".to_string(),
            }),
        )
    })?;

    let listings = reader.get_active_listings().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "ReadError".to_string(),
                message: format!("Failed to read listings: {}", e),
            }),
        )
    })?;

    // Enrich listings with NFT metadata and platform detection
    let mut enriched = Vec::with_capacity(listings.len());
    // Cache collection info per contract to avoid redundant RPC calls
    let mut collection_cache: std::collections::HashMap<String, (String, String)> = std::collections::HashMap::new();

    for listing in listings {
        let nft_lower = listing.nft_contract.to_lowercase();

        let platform = if state.sablier_contracts.iter().any(|c| c.to_lowercase() == nft_lower) {
            Some("sablier".to_string())
        } else if state.hedgey_contracts.iter().any(|c| c.to_lowercase() == nft_lower) {
            Some("hedgey".to_string())
        } else {
            None
        };

        let (name, symbol) = if let Some(cached) = collection_cache.get(&nft_lower) {
            cached.clone()
        } else if let Some(nft) = state.nft_reader.as_ref() {
            let info = nft.get_collection_info(&listing.nft_contract).await;
            collection_cache.insert(nft_lower.clone(), info.clone());
            info
        } else {
            (String::new(), String::new())
        };

        enriched.push(EnrichedListing {
            listing,
            nft_name: name,
            nft_symbol: symbol,
            platform,
        });
    }

    let total = enriched.len();
    Ok(Json(ListingsResponse { listings: enriched, total }))
}

pub async fn get_listing_detail(
    State(state): State<Arc<ApiState>>,
    Path(listing_id): Path<u64>,
) -> Result<Json<ListingDetailResponse>, (StatusCode, Json<ErrorResponse>)> {
    let escrow = state.escrow_reader.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "EscrowNotConfigured".to_string(),
                message: "Escrow reader not configured".to_string(),
            }),
        )
    })?;

    let listing = escrow.get_listing(listing_id).await.map_err(|e| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "ListingNotFound".to_string(),
                message: format!("Listing {} not found: {}", listing_id, e),
            }),
        )
    })?;

    // Always try to get generic NFT metadata
    let nft = if let Some(nft_reader) = state.nft_reader.as_ref() {
        nft_reader.get_metadata(&listing.nft_contract, listing.token_id).await.ok()
    } else {
        None
    };

    // Try to fetch vesting info for known platform NFTs
    let vesting = if let Some(reader) = state.vesting_reader.as_ref() {
        let nft_lower = listing.nft_contract.to_lowercase();
        let is_sablier = state.sablier_contracts.iter().any(|c| c.to_lowercase() == nft_lower);
        let is_hedgey = state.hedgey_contracts.iter().any(|c| c.to_lowercase() == nft_lower);
        let escrow_addr = escrow.contract_address();

        if is_sablier {
            reader.get_sablier_positions(&listing.nft_contract, escrow_addr)
                .await
                .ok()
                .and_then(|positions| {
                    positions.into_iter().find(|p| p.token_id == listing.token_id)
                })
        } else if is_hedgey {
            reader.get_hedgey_positions(&listing.nft_contract, escrow_addr)
                .await
                .ok()
                .and_then(|positions| {
                    positions.into_iter().find(|p| p.token_id == listing.token_id)
                })
        } else {
            None
        }
    } else {
        None
    };

    Ok(Json(ListingDetailResponse { listing, nft, vesting }))
}

/// Discover NFTs owned by address from arbitrary contracts
pub async fn get_nfts(
    State(state): State<Arc<ApiState>>,
    Path(address): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<NftDiscoveryResponse>, (StatusCode, Json<ErrorResponse>)> {
    let nft_reader = state.nft_reader.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "NftReaderNotConfigured".to_string(),
                message: "NFT reader not configured".to_string(),
            }),
        )
    })?;

    let clean_addr = if address.starts_with("0x") {
        address.clone()
    } else {
        format!("0x{}", address)
    };

    // Get contracts to scan from query param: ?contracts=0x...,0x...
    let contracts: Vec<String> = params.get("contracts")
        .map(|c| c.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();

    if contracts.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "MissingContracts".to_string(),
                message: "Provide ?contracts=0x...,0x... to scan for NFTs".to_string(),
            }),
        ));
    }

    // Cap at 10 contracts per request
    let mut all_nfts = Vec::new();
    for contract in contracts.iter().take(10) {
        match nft_reader.get_owned_nfts(contract, &clean_addr).await {
            Ok(nfts) => all_nfts.extend(nfts),
            Err(e) => tracing::warn!("Failed to scan NFTs from {}: {}", contract, e),
        }
    }

    let total = all_nfts.len();
    Ok(Json(NftDiscoveryResponse {
        address: clean_addr,
        nfts: all_nfts,
        total,
    }))
}

// ══════════════════════════════════════════════
// ██  CROSS-CHAIN NFT MARKETPLACE
// ══════════════════════════════════════════════

/// GET /api/v1/nft-listings — list all NFT listings from sequencer state
pub async fn get_nft_listings(
    State(state): State<Arc<ApiState>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let sequencer_state = state.sequencer.get_state();
    let state_guard = sequencer_state.lock().unwrap();

    let status_filter = params.get("status").map(|s| s.as_str());

    let listings: Vec<serde_json::Value> = state_guard
        .nft_listings
        .values()
        .filter(|l| match status_filter {
            Some("active") => l.status == axync_types::NftListingStatus::Active,
            Some("sold") => l.status == axync_types::NftListingStatus::Sold,
            Some("cancelled") => l.status == axync_types::NftListingStatus::Cancelled,
            _ => true,
        })
        .map(|l| {
            serde_json::json!({
                "id": l.id,
                "seller": format!("0x{}", hex::encode(l.seller)),
                "nft_contract": format!("0x{}", hex::encode(l.nft_contract)),
                "token_id": l.token_id,
                "nft_chain_id": l.nft_chain_id,
                "price": l.price.to_string(),
                "payment_chain_id": l.payment_chain_id,
                "status": format!("{:?}", l.status),
                "buyer": if l.buyer == axync_types::ZERO_ADDRESS {
                    None
                } else {
                    Some(format!("0x{}", hex::encode(l.buyer)))
                },
                "on_chain_listing_id": l.on_chain_listing_id,
                "created_at": l.created_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "listings": listings,
        "total": listings.len(),
    })))
}

/// GET /api/v1/nft-listing/:listing_id — get single listing details
pub async fn get_nft_listing(
    State(state): State<Arc<ApiState>>,
    Path(listing_id): Path<u64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let sequencer_state = state.sequencer.get_state();
    let state_guard = sequencer_state.lock().unwrap();

    let listing = state_guard.get_nft_listing(listing_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "NotFound".to_string(),
                message: format!("Listing {} not found", listing_id),
            }),
        )
    })?;

    Ok(Json(serde_json::json!({
        "id": listing.id,
        "seller": format!("0x{}", hex::encode(listing.seller)),
        "nft_contract": format!("0x{}", hex::encode(listing.nft_contract)),
        "token_id": listing.token_id,
        "nft_chain_id": listing.nft_chain_id,
        "price": listing.price.to_string(),
        "payment_chain_id": listing.payment_chain_id,
        "status": format!("{:?}", listing.status),
        "buyer": if listing.buyer == axync_types::ZERO_ADDRESS {
            None
        } else {
            Some(format!("0x{}", hex::encode(listing.buyer)))
        },
        "on_chain_listing_id": listing.on_chain_listing_id,
        "created_at": listing.created_at,
    })))
}

/// GET /api/v1/nft-release-proof/:listing_id — merkle proof for claimNft on-chain
pub async fn get_nft_release_proof(
    State(state): State<Arc<ApiState>>,
    Path(listing_id): Path<u64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let sequencer_state = state.sequencer.get_state();
    let state_guard = sequencer_state.lock().unwrap();

    let listing = state_guard.get_nft_listing(listing_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "NotFound".to_string(),
                message: format!("Listing {} not found", listing_id),
            }),
        )
    })?;

    if listing.status != axync_types::NftListingStatus::Sold {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "NotSold".to_string(),
                message: "Listing has not been sold yet".to_string(),
            }),
        ));
    }

    // Compute release leaf based on asset type
    let leaf = match listing.asset_type {
        axync_types::AssetType::ERC721 => axync_prover::merkle::hash_nft_release(
            listing.nft_contract,
            listing.token_id,
            listing.buyer,
            listing.nft_chain_id,
            listing.on_chain_listing_id,
        ),
        axync_types::AssetType::ERC20 => axync_prover::merkle::hash_token_release(
            listing.nft_contract,
            listing.amount,
            listing.buyer,
            listing.nft_chain_id,
            listing.on_chain_listing_id,
        ),
    };

    // Compute nullifier (keccak256)
    let nullifier = {
        use sha3::{Digest, Keccak256};
        let mut hasher = Keccak256::new();
        hasher.update(&leaf);
        hasher.update(&listing.on_chain_listing_id.to_le_bytes());
        let result: [u8; 32] = hasher.finalize().into();
        result
    };

    let listing_id_copy = listing.id;
    let on_chain_listing_id = listing.on_chain_listing_id;
    let buyer = listing.buyer;
    let nft_contract = listing.nft_contract;
    let token_id = listing.token_id;
    let nft_chain_id = listing.nft_chain_id;
    drop(state_guard);

    // Generate real merkle proof by loading the block and rebuilding the tree
    let (proof_siblings, _root) = state.sequencer.generate_nft_release_proof(listing_id_copy)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "ProofError".to_string(),
                    message: format!("Failed to generate merkle proof: {:?}", e),
                }),
            )
        })?;

    // Serialize proof as concatenated 32-byte siblings (matches Solidity bytes calldata)
    let merkle_proof_hex = format!("0x{}", proof_siblings.iter()
        .map(|s| hex::encode(s))
        .collect::<String>());

    Ok(Json(serde_json::json!({
        "listing_id": listing_id_copy,
        "on_chain_listing_id": on_chain_listing_id,
        "buyer": format!("0x{}", hex::encode(buyer)),
        "nft_contract": format!("0x{}", hex::encode(nft_contract)),
        "token_id": token_id,
        "nft_chain_id": nft_chain_id,
        "leaf": format!("0x{}", hex::encode(leaf)),
        "nullifier": format!("0x{}", hex::encode(nullifier)),
        "merkle_proof": merkle_proof_hex,
    })))
}

/// GET /api/v1/withdrawal-proof/:address/:asset_id/:amount/:chain_id
pub async fn get_withdrawal_proof(
    State(state): State<Arc<ApiState>>,
    Path((address, asset_id, amount, chain_id)): Path<(String, u16, String, u64)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let address_clean = address.strip_prefix("0x").unwrap_or(&address);
    let address_bytes: [u8; 20] = hex::decode(address_clean)
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: "InvalidAddress".to_string(),
            message: "Invalid address format".to_string(),
        })))?
        .try_into()
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: "InvalidAddress".to_string(),
            message: "Address must be 20 bytes".to_string(),
        })))?;

    let amount_u128: u128 = amount.parse().map_err(|_| (StatusCode::BAD_REQUEST, Json(ErrorResponse {
        error: "InvalidAmount".to_string(),
        message: "Invalid amount".to_string(),
    })))?;

    // Compute the withdrawal leaf (keccak256, ABI-compatible)
    let leaf = axync_prover::merkle::hash_withdrawal(
        address_bytes,
        asset_id,
        amount_u128,
        chain_id,
    );

    // Compute nullifier
    let nullifier = {
        use sha3::{Digest, Keccak256};
        let mut hasher = Keccak256::new();
        hasher.update(&leaf);
        hasher.update(&address_bytes);
        hasher.update(&chain_id.to_le_bytes());
        let result: [u8; 32] = hasher.finalize().into();
        result
    };

    // Generate real merkle proof
    let (proof_siblings, _root) = state.sequencer.generate_withdrawal_proof(
        &address_bytes, asset_id, amount_u128, chain_id
    ).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "ProofError".to_string(),
                message: format!("Failed to generate merkle proof: {:?}", e),
            }),
        )
    })?;

    let merkle_proof_hex = format!("0x{}", proof_siblings.iter()
        .map(|s| hex::encode(s))
        .collect::<String>());

    Ok(Json(serde_json::json!({
        "user": format!("0x{}", hex::encode(address_bytes)),
        "asset_id": asset_id,
        "amount": amount,
        "chain_id": chain_id,
        "leaf": format!("0x{}", hex::encode(leaf)),
        "nullifier": format!("0x{}", hex::encode(nullifier)),
        "merkle_proof": merkle_proof_hex,
    })))
}
