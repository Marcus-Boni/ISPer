//! O cliente contra um OptTime de mentira: HTTP de verdade em 127.0.0.1, com
//! as respostas no formato do MCP hospedado (sem sessão, JSON no corpo, GET
//! recusado com 405, erro de ferramenta com o código em `_meta`).

// O servidor falso é código de teste fora de um #[test].
#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use isper_mcp::{ApplyItem, ApplyRequest, Endpoint, McpError, OptTime};
use serde_json::{Value, json};

const TOKEN: &str = "opt_tok_de_teste";

/// As chamadas de ferramenta que o servidor recebeu (nome, argumentos).
type Calls = Arc<Mutex<Vec<(String, Value)>>>;

struct Fake {
    url: String,
    calls: Calls,
}

fn start(delay: Duration) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/api/mcp", listener.local_addr().unwrap());
    let calls: Calls = Arc::default();
    let seen = calls.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let seen = seen.clone();
            std::thread::spawn(move || serve(stream, &seen, delay));
        }
    });
    Fake { url, calls }
}

fn serve(mut stream: TcpStream, calls: &Calls, delay: Duration) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut length = 0usize;
    let mut auth = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        let (name, value) = line.split_once(':').unwrap_or((&line, ""));
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => length = value.trim().parse().unwrap_or(0),
            "authorization" => auth = value.trim().to_string(),
            _ => {}
        }
    }
    let mut body = vec![0; length];
    let _ = reader.read_exact(&mut body);

    let method = request_line.split_whitespace().next().unwrap_or("");
    let (status, payload) = match method {
        "GET" => ("405 Method Not Allowed", None),
        "DELETE" => ("204 No Content", None),
        "POST" if auth != format!("Bearer {TOKEN}") => (
            "401 Unauthorized",
            Some(json!({ "error": { "code": "UNAUTHORIZED", "message": "token inválido" } })),
        ),
        "POST" => {
            let msg: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            match msg.get("id").cloned() {
                None => ("202 Accepted", None),
                Some(id) => {
                    std::thread::sleep(delay);
                    let result = answer(&msg, calls);
                    (
                        "200 OK",
                        Some(json!({ "jsonrpc": "2.0", "id": id, "result": result })),
                    )
                }
            }
        }
        _ => ("400 Bad Request", None),
    };
    let body = payload.map(|p| p.to_string()).unwrap_or_default();
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    if status.starts_with("401") {
        head.push_str("WWW-Authenticate: Bearer realm=\"opt-time\"\r\n");
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn answer(msg: &Value, calls: &Calls) -> Value {
    let params = &msg["params"];
    match msg["method"].as_str().unwrap_or("") {
        "initialize" => json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "opt-time", "version": "1.11.0" }
        }),
        "tools/list" => json!({ "tools": [
            {
                "name": "opt_time_whoami", "title": "Identificar usuário",
                "inputSchema": { "type": "object", "properties": {} },
                "annotations": { "readOnlyHint": true }
            },
            {
                "name": "opt_time_apply_suggestions",
                "inputSchema": { "type": "object", "properties": {} },
                "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true }
            },
            { "name": "opt_time_delete_time_entry", "inputSchema": { "type": "object" } }
        ]}),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            calls.lock().unwrap().push((name.clone(), args.clone()));
            tool(&name, &args)
        }
        other => json!({ "unexpected": other }),
    }
}

fn tool(name: &str, args: &Value) -> Value {
    let ok = |data: Value| json!({ "content": [{ "type": "text", "text": "ok" }], "structuredContent": data });
    match name {
        "opt_time_whoami" => ok(json!({
            "userId": "u1", "name": "Fulano de Tal", "email": "fulano@example.com",
            "role": "member", "scopes": ["time:read", "time:write", "calendar:read"],
            "tokenName": "ISPer", "timezone": "America/Sao_Paulo",
            "weeklyCapacityMinutes": 2400,
            "today": { "date": "2026-10-08", "totalMinutes": 280, "dailyCapacityMinutes": 480 },
            "microsoft": { "connected": true, "needsReconnect": false, "tokenUsable": true },
            "azureDevOps": { "configured": false },
            "eveningDigestEnabled": true
        })),
        "opt_time_get_today_summary" => ok(json!({
            "date": args["date"].as_str().unwrap_or("2026-10-08"),
            "weekday": "quinta-feira", "totalMinutes": 280, "totalLabel": "4h40",
            "billableMinutes": 280, "entryCount": 2, "dailyCapacityMinutes": 480,
            "remainingMinutes": 200, "remainingLabel": "3h20", "isComplete": false,
            "byProject": [{ "projectId": "p1", "projectName": "Portal", "projectCode": "OPT-001", "minutes": 280, "label": "4h40" }],
            "entries": [], "activeTimer": null, "weekTotalMinutes": 1720,
            "weeklyCapacityMinutes": 2400, "isWorkday": true, "targetMinutes": 480,
            "warnings": []
        })),
        "opt_time_suggest_daily_entries" => ok(json!({
            "date": "2026-10-08",
            "suggestions": [{
                "id": "sug-1", "source": "calendar", "sourceRef": "evt-1",
                "projectId": "p1", "projectName": "Portal",
                "description": "Refinamento do backlog", "date": "2026-10-08",
                "startsAt": "2026-10-08T14:00:00-03:00", "durationMinutes": 60,
                "durationLabel": "1h", "billable": true, "azureWorkItemId": null,
                "confidence": "high", "evidence": "Reunião aceita no Outlook", "reasons": []
            }, {
                "id": "sug-2", "source": "voice_memo", "sourceRef": null,
                "projectId": null, "projectName": null,
                "description": "Algo de uma fonte nova", "date": "2026-10-08",
                "durationMinutes": 30, "durationLabel": "30min", "billable": false,
                "confidence": "low", "evidence": "fonte que o ISPer não conhece"
            }],
            "alreadyLoggedMinutes": 280, "alreadyLoggedLabel": "4h40",
            "targetMinutes": 480, "gapMinutes": 200,
            "sources": { "outlook": true, "teamsCalls": true, "azureDevOps": false, "history": true, "commits": 0 },
            "warnings": [], "notes": []
        })),
        "opt_time_apply_suggestions" => ok(json!({
            "date": args["date"], "createdEntryIds": ["e1"],
            "dayTotalMinutes": 340, "dailyCapacityMinutes": 480,
            "remainingMinutes": 140, "replayed": false
        })),
        _ => json!({
            "content": [{ "type": "text", "text": "❌ Conta Microsoft não conectada." }],
            "_meta": { "opt-time/error": {
                "code": "MICROSOFT_NOT_CONNECTED",
                "message": "Conta Microsoft não conectada.",
                "hint": "Entre no OptTime com a conta Microsoft.",
                "details": null
            }},
            "isError": true
        }),
    }
}

fn opttime(fake: &Fake) -> OptTime {
    OptTime::new(Endpoint::new(&fake.url, TOKEN).unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn whoami_resumo_e_sugestoes() {
    let fake = start(Duration::ZERO);
    let ot = opttime(&fake);

    let me = ot.whoami().await.unwrap();
    assert_eq!(me.name, "Fulano de Tal");
    assert!(me.missing_scopes().is_empty());
    assert!(me.microsoft.token_usable);
    assert!(!me.azure_dev_ops.configured);

    let day = ot.day_summary(Some("2026-10-08")).await.unwrap();
    assert_eq!(day.missing_minutes(), 200);
    assert_eq!(day.by_project[0].project_code, "OPT-001");

    let s = ot.suggest(None).await.unwrap();
    assert_eq!(s.suggestions.len(), 2);
    assert_eq!(
        s.suggestions[1].source, "voice_memo",
        "fonte nova não quebra"
    );
    assert_eq!(s.suggestions[1].project_id, None);

    let calls = fake.calls.lock().unwrap().clone();
    let names: Vec<&str> = calls.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "opt_time_whoami",
            "opt_time_get_today_summary",
            "opt_time_suggest_daily_entries"
        ]
    );
    assert_eq!(calls[1].1, json!({ "date": "2026-10-08" }));
    assert_eq!(calls[2].1, json!({}));
}

#[tokio::test(flavor = "multi_thread")]
async fn aplicar_manda_a_chave_de_idempotencia() {
    let fake = start(Duration::ZERO);
    let req = ApplyRequest::new(
        "2026-10-08",
        vec![ApplyItem {
            suggestion_id: "sug-1".into(),
            project_id: None,
            duration_minutes: None,
            description: Some("Refinamento".into()),
            billable: None,
        }],
        vec!["sug-2".into()],
    );
    let applied = opttime(&fake).apply(&req).await.unwrap();
    assert_eq!(applied.created_entry_ids, ["e1"]);
    assert_eq!(applied.remaining_minutes, 140);
    let calls = fake.calls.lock().unwrap().clone();
    let (name, args) = &calls[0];
    assert_eq!(name, "opt_time_apply_suggestions");
    assert_eq!(args["idempotencyKey"], req.idempotency_key);
    assert_eq!(
        args["items"],
        json!([{ "suggestionId": "sug-1", "description": "Refinamento" }])
    );
    assert_eq!(args["rejectedSuggestionIds"], json!(["sug-2"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn erro_de_ferramenta_traz_codigo_e_dica() {
    let fake = start(Duration::ZERO);
    let endpoint = Endpoint::new(&fake.url, TOKEN).unwrap();
    let err = endpoint
        .call("opt_time_get_my_agenda", json!({}))
        .await
        .unwrap_err();
    assert_eq!(err.code(), "MICROSOFT_NOT_CONNECTED");
    assert_eq!(err.hint(), Some("Entre no OptTime com a conta Microsoft."));
}

#[tokio::test(flavor = "multi_thread")]
async fn catalogo_com_as_marcacoes() {
    let fake = start(Duration::ZERO);
    let conn = Endpoint::new(&fake.url, TOKEN)
        .unwrap()
        .connect()
        .await
        .unwrap();
    assert_eq!(
        conn.server(),
        Some(("opt-time".to_string(), "1.11.0".to_string()))
    );
    let tools = conn.tools().await.unwrap();
    conn.close().await;
    let by = |n: &str| tools.iter().find(|t| t.name == n).unwrap();
    assert!(by("opt_time_whoami").read_only);
    assert!(!by("opt_time_whoami").destructive);
    let apply = by("opt_time_apply_suggestions");
    assert!(!apply.read_only && !apply.destructive && apply.idempotent);
    assert!(
        by("opt_time_delete_time_entry").destructive,
        "sem marcação, o MCP manda supor que apaga"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn token_errado_e_recusado() {
    let fake = start(Duration::ZERO);
    let ot = OptTime::new(Endpoint::new(&fake.url, "opt_tok_errado").unwrap());
    let err = ot.whoami().await.unwrap_err();
    assert!(matches!(err, McpError::Unauthorized), "veio {err:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn servidor_lento_estoura_o_prazo() {
    let fake = start(Duration::from_secs(3));
    let ot = OptTime::new(
        Endpoint::new(&fake.url, TOKEN)
            .unwrap()
            .with_timeout(Duration::from_millis(500)),
    );
    let err = ot.whoami().await.unwrap_err();
    assert!(matches!(err, McpError::Timeout(_)), "veio {err:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn servidor_fora_do_ar() {
    // Uma porta que ninguém escuta.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let ot = OptTime::new(
        Endpoint::new(&format!("http://127.0.0.1:{port}/api/mcp"), TOKEN)
            .unwrap()
            .with_timeout(Duration::from_secs(5)),
    );
    let err = ot.whoami().await.unwrap_err();
    assert!(matches!(err, McpError::Transport(_)), "veio {err:?}");
}
