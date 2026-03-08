use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use std::collections::HashMap;
use zkclear_types::{DealVisibility, TxKind, TxPayload};
use std::sync::Arc;
use zkclear_sequencer::Sequencer;
use zkclear_storage::Storage;
use zkclear_types::{AssetId, BlockId, DealId};

use crate::types::*;
use zkclear_sequencer::security::{sanitize_string, validate_hex_string};

pub struct ApiState {
    pub sequencer: Arc<Sequencer>,
    pub storage: Option<Arc<dyn Storage>>,
    pub rate_limit_state: Option<Arc<crate::middleware::RateLimitState>>,
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

    let balance = account
        .balances
        .iter()
        .find(|b| b.asset_id == asset_id)
        .map(|b| (b.chain_id, b.amount))
        .unwrap_or((zkclear_types::chain_ids::ETHEREUM, 0));

    Ok(Json(AccountBalanceResponse {
        address: addr,
        asset_id,
        chain_id: balance.0,
        amount: balance.1,
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
                && matches!(deal.status, zkclear_types::DealStatus::Pending)
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
            asset_base: deal.asset_base,
            asset_quote: deal.asset_quote,
            chain_id_base: deal.chain_id_base,
            chain_id_quote: deal.chain_id_quote,
            amount_base: deal.amount_base,
            amount_remaining: deal.amount_remaining,
            price_quote_per_base: deal.price_quote_per_base,
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
        asset_base: deal.asset_base,
        asset_quote: deal.asset_quote,
        chain_id_base: deal.chain_id_base,
        chain_id_quote: deal.chain_id_quote,
        amount_base: deal.amount_base,
        amount_remaining: deal.amount_remaining,
        price_quote_per_base: deal.price_quote_per_base,
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
        transactions,
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
                "chain_id": zkclear_types::chain_ids::ETHEREUM,
                "name": "Ethereum"
            },
            {
                "chain_id": zkclear_types::chain_ids::POLYGON,
                "name": "Polygon"
            },
            {
                "chain_id": zkclear_types::chain_ids::BASE,
                "name": "Base"
            },
            {
                "chain_id": zkclear_types::chain_ids::ARBITRUM,
                "name": "Arbitrum"
            },
            {
                "chain_id": zkclear_types::chain_ids::OPTIMISM,
                "name": "Optimism"
            },
            {
                "chain_id": zkclear_types::chain_ids::BASE,
                "name": "Base"
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

            let tx: zkclear_types::Tx = match bincode::deserialize(&tx_bytes) {
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
                Err(zkclear_sequencer::SequencerError::QueueFull) => {
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
                Err(zkclear_sequencer::SequencerError::InvalidSignature) => {
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
                Err(zkclear_sequencer::SequencerError::InvalidNonce) => {
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
    use zkclear_types::Tx;

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
                payload: TxPayload::Deposit(zkclear_types::Deposit {
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
            asset_base,
            asset_quote,
            chain_id_base,
            chain_id_quote,
            amount_base,
            price_quote_per_base,
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
                payload: TxPayload::CreateDeal(zkclear_types::CreateDeal {
                    deal_id,
                    visibility: visibility_enum,
                    taker: taker_addr,
                    asset_base,
                    asset_quote,
                    chain_id_base,
                    chain_id_quote,
                    amount_base,
                    price_quote_per_base,
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
                payload: TxPayload::AcceptDeal(zkclear_types::AcceptDeal {
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
                payload: TxPayload::CancelDeal(zkclear_types::CancelDeal { deal_id }),
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
                payload: TxPayload::Withdraw(zkclear_types::Withdraw {
                    asset_id,
                    amount,
                    to: to_address,
                    chain_id,
                }),
                signature: sig,
            };

            (tx, from_address)
        }
    };

    // Serialize transaction before submitting (for tx_hash generation)
    let tx_hash = hex::encode(&bincode::serialize(&tx).unwrap_or_default());
    
    match state.sequencer.submit_tx_with_validation(tx, false) {
        Ok(()) => {
            Ok(Json(crate::types::SubmitTransactionResponse {
                tx_hash,
                status: "queued".to_string(),
            }))
        }
        Err(zkclear_sequencer::SequencerError::QueueFull) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "QueueFull".to_string(),
                message: "Transaction queue is full".to_string(),
            }),
        )),
        Err(zkclear_sequencer::SequencerError::InvalidSignature) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "InvalidSignature".to_string(),
                message: "Transaction signature is invalid".to_string(),
            }),
        )),
        Err(zkclear_sequencer::SequencerError::InvalidNonce) => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "InvalidNonce".to_string(),
                message: "Transaction nonce is invalid".to_string(),
            }),
        )),
        Err(zkclear_sequencer::SequencerError::ExecutionFailed(stf_err)) => {
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
