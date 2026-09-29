# M0 배포·키·관측·복구 설계

## 로컬 4검증인 및 재현 배포

`topology.json`의 val1–4는 동일 투표권 1, 독립 home, 분리 RPC/P2P port와 논리 FD-1–4를 가진다. RPC는 loopback 전용이다. 로컬에서는 모두 같은 전원·호스트·디스크이므로 독립 장애 영역이 아니다. 실제 배치 제안은 독립 FD 4곳에 검증인 하나씩, 거래소 active/standby는 두 영역, 별도 비검증 full RPC 두 곳과 독립 비상 RPC/후원이다. 검증인 RPC, WAL, DB를 외부 노출하지 않는다. sentry/mTLS/방화벽 allowlist 적용 여부는 후속 실제 환경 시험 대상이다.

Chain adapter가 정해지면 다음 절차를 실행한다. 현재 모의 manifest를 그대로 genesis로 사용하면 안 된다.

1. release manifest에 chain_id, genesis hash, binary/image digest, SDK/CometBFT/Go 버전과 소스 commit, protocol/vector hash, migration ID를 고정한다. 값이 없거나 hash 불일치면 배포 중단.
2. Chain 제공 CLI로 각각 독립 test key를 생성하고 4개 gentx를 하나의 genesis에 수집한다. 각 home의 genesis byte hash와 동일 power를 확인한다. role별 signer reference만 주입하고 이미지에 키를 bake하지 않는다.
3. Paperclip execution workspace에 네 validator를 foreground로 관리하는 supervisor command를 등록한다. supervisor는 자식 exit 전달, SIGTERM 전달·대기, stdout 로그를 제공해야 한다. 서비스 health는 포트 열림 외에 동일 chain/genesis, 높이 증가, app hash 및 peer 연결을 확인한다.
4. `SRE_WORKSPACE_COMMAND_ID`를 명시해 `bash ops/runtime.sh start` 실행. start/stop/restart는 관리 API만 사용한다. 실제 서비스가 생기면 `runtime_service` work product에 runtime ID·URL·명령·health를 등록한다.
5. 4대→1대 중지에서 높이 증가, 2대 중지에서 확정 중단, 복귀 시 app hash 수렴을 실제 시험한다. 현재는 3/4 quorum 산술만 검증했다.
6. 종료 시 신규 주문 차단 → ACK frontier/outbox 저장 → 정산 상황 저장 → 관리 stop → 프로세스 종료 확인. 정상 재시작은 기존 signer state를 보존한다. 새 genesis 재배포는 새 chain_id와 새 모의 키·새 home을 사용한다.
7. binary rollback은 schema/체인 업그레이드 호환성이 증명된 경우만 한다. 불명확하면 쓰기를 멈추고 forward fix. home 덮어쓰기, signer state rewind, 동일 키 두 signer 동시 실행 금지.

## 키 경계

| 키/상태 | 허용 주체 | 격리와 복원 조건 |
|---|---|---|
| 사용자 ML-DSA | 사용자 단말 | 서버 전달 금지; 백업·복구도 단말 경계 |
| validator consensus | 검증인별 전용 signer | 각기 다른 자격·volume; 마지막 서명 높이/round/step 함께 보호; 구 signer 차단 증명 후 새 signer 허용 |
| P2P node key | 해당 노드 | consensus 키와 별개; 유출 시 신원 회전 절차 |
| 발행/소각 | 발행 승인자 | relayer/sponsor와 권한 분리, 복수 승인; hot 운영 키에 mint 권한 금지 |
| relayer | 단일 제출자 | 현재 operator_epoch만 허용; 사용자 서명 대체 불가 |
| sponsor / emergency sponsor | 각각 다른 운영 경로 | 메시지·금액·기간 한도; 별도 계정/예산·접근권한 |
| upgrade/admin | 관리 승인자 | 발행·relay와 분리, 복수 승인·변경 감사 |

실험은 9개 역할 marker의 고유성, 0700 home/0600 파일을 검사했다. 동일 OS 사용자 간 접근 격리는 보장하지 않는다. 실제 검증은 별도 UID/container/volume/secret ACL로 다른 역할 read/sign 요청을 거절하는지 수행한다. signer 운영에 어떤 KMS/HSM이 ML-DSA를 지원한다고 가정하지 않는다. 제품·버전 확정 후 생성/복구/서명/권한 거절을 검증한다.

## WAL·승격·leader 차단

ACK 조건 제안: 단일 시퀀서의 주문/취소·예약 변화·결정적 outbox를 하나의 원자 기록으로 durable commit하고 다른 장애 영역의 복제 확인 후 응답한다. 복제 quorum을 잃으면 신규 ACK를 중지한다. 재시도는 (owner, epoch, order_id)와 본문 hash로 결합한다. ACK 응답이 유실된 기록도 재생될 수 있다.

승격 순서: 접수 중지 → 구 리더 storage write/서명 권한 차단 및 차단 확인 → 단조 증가 fencing epoch 발급 → 복제된 commit frontier까지 재생 → 체인 LastBatch/seq/hash/operator_epoch 대조 → outbox 재구성 → 원장 검증 → 새 리더 쓰기 허용. fencing은 모든 실제 쓰기와 signer에서 token을 원자적으로 검사해야 한다. lease나 PID 파일만으로 충분하다고 보지 않는다. 차단 확인 불가 시 가용성보다 안전을 우선해 승격하지 않는다.

이번 serial epoch 모형은 조건문이 옛 epoch를 거절하는지만 입증한다. 네트워크 분할, 동시에 실행되는 두 writer, 저장소의 조건부 쓰기, chain operator epoch, 실제 늦은 배치 거절은 Exchange/Chain 구현을 연결한 후 시험해야 한다. 파일 복제 실험은 실제 WAL/outbox transaction 구현의 증거가 아니다.

## 관측과 임시 경보 제안

아래 threshold는 실험 시작값으로 CTO/Tester 검토 대상이다. 사용자 ID·원문 서명·키는 metric label에 넣지 않는다.

| 영역 | 필수 계측 | 임시 trigger / 대응 |
|---|---|---|
| 합의 | height, finalized latency, validator participation, RPC height/app hash | 3 block interval 동안 증가 없음: 운영 호출·신규 주문 차단, 합의 중단 표시 |
| WAL | local/durable/replica/ACK seq, fsync latency, replica lag | ACK > durable frontier 또는 accepted stale epoch 1건: 즉시 안전 정지 |
| 단일 리더 | leader epoch, writer count, fencing failure | writer>1 또는 차단 미확인: 모든 신규 접수/승격 차단 |
| 정산 | last seq/hash, pending oldest age, reject reason, gas | 체인-로컬 hash 불일치 즉시 정산 중지; pending age는 DEC-05 기준 연결 |
| 보존식 | bank-(SUM C+T+U), min C, min(C-R-D), U | 음수·보존 위반 1건: 정산/신규 주문 차단·사고 기록. U>0 격리·조사, 정당한 직접 출금 유지 |
| 조회 | chain/indexer height gap, event cursor, WS gap | 2 block 이상 지연: stale 표시·새 주문 위험 검사 중지; 재연결 snapshot |
| 비상 | RPC health/height, sponsor grant expiry/budget, native balance | 최소 2회 모의 출금 가스 미만 또는 만료 24시간 전 경보(정책 미확정) |
| 백업 | last verified height/seq, age, restore hash | 백업 검증 실패 즉시 경보; 훈련 누락은 운영 진입 gate 실패 |

안전 정지는 새 주문만 중단하며 유효 미정산 배치를 처리할 수 있다. 정산 정지는 신규 배치를 막고 사용자 취소·직접 회수 경로를 유지한다. 합의 중단은 확정 TX 자체가 불가능하므로 직접 출금도 완료되지 않는다. 상태와 경보 전달 실패 자체를 독립 채널에서 관측한다.

## 백업·복구 runbook

백업 manifest는 chain/genesis hash, app/schema/protocol 버전, finalized height/app hash, WAL snapshot seq/commit frontier, checksum, indexer cursor, LastBatch seq/hash를 포함한다. 체인 상태·WAL snapshot/segments·인덱서·설정은 독립 암호화 백업한다. encryption key는 데이터 저장소와 다른 권한으로 보관하고 escrow 복구 훈련을 한다. signer secret/state는 일반 데이터 snapshot에 섞거나 자동 rollback하지 않는다.

복구 절차: 서비스 쓰기 중단 및 구 writer 차단 → manifest/checksum 검증 → 빈 격리 환경에 복원 → snapshot과 연속 WAL 범위 검사 → committed frontier까지만 재생 → 체인 확정 높이 기준 C/R/D/P 재계산 → 보존식·outbox·LastBatch 대사 → 인덱서 재구축 → 제한된 모의 주문/직접 출금 → 운영자 검토 후 접수 재개. 불연속/완성 레코드 손상은 자동 건너뛰지 않는다. 미완성 꼬리는 ACK frontier를 충족하는지 확인해야 폐기할 수 있다.

현재 실험은 JSONL 전체 복사·hash·재생만 검증한다. 암호화, storage flush의 전원 손실 보장, offsite 보존/삭제 정책, 체인 snapshot, 실제 signer 복구는 미검증이다. 보존 기간은 DEC-11 및 운영 정책으로 확정한다.

## 장애·비상 RPC/가스 시험 행렬

| ID | 주입/입력 | 통과 증거 | 이번 상태 / 다음 책임 |
|---|---|---|---|
| F01 / T08 | 100 합성 기록, local fsync/replica fsync/ACK 직후 종료 | 외부 ACK 집합 포함, replay/outbox hash 동일 | 파일 모형 PASS; 실제 Exchange WAL 연결 SRE+Exchange |
| F02 / T10 | 네트워크 양분, 구 leader 재진입, 늦은 배치 | 단일 writer, 구 epoch 쓰기·서명·체인 제출 거절 | serial 조건 PASS, 분산 실험 미실행; Exchange/Chain/SRE |
| F03 | 검증인 1대/2대 종료·복귀 | 1대 손실 진행, 2대 손실 중단, 복귀 동일 app hash | 산술만 PASS; Chain adapter 후 SRE |
| F04 | 백업 byte 변조/완성 WAL 변조/잘린 tail | checksum 거절/온전한 prefix 재생, ACK 유실 0 | 파일 모형 PASS; 실제 snapshot 시험 필요 |
| F05 / T13 | 거래소·relay·기본 sponsor와 RPC 차단 | 독립 RPC에서 사용자 서명 epoch 변경/취소 → 확정 잔고 직접 출금; 실제 finalized receipt | 미실행; Chain/Wallet/SRE, Security/Tester 검토 |
| F06 / T12 | 후원 만료·소진·허용 외 메시지·native 0 | 가치 변화 없이 거절; 별도 제한 후원 또는 사용자 native 경로 성공 | 미실행; Chain 정책/모의 genesis 자금 필요 |
| F07 | 비상 RPC stale/다른 genesis·2 validators down | stale/다른 체인 거절, 합의 중단 중 출금 성공 오표시 없음 | 미실행; Chain/SRE |
| F08 | consensus key+서명 상태 복원, 구 signer 격리 실패 | 재사용·동시 활성 차단; last-sign-state rewind 거절 | 미실행; Chain/Security/SRE |
| F09 / T04,T07,T15 | 직접 출금/정산 두 순서, 중복 batch, U+/부족 | 음수·중복 지급 0, 정산 정지와 직접 회수 구분 | 제품 구현 미실행; Chain/Settlement/Tester |

모든 실제 시험은 모의 키·합성 자산만 사용한다. 비상 경로는 기본 운영자·DNS·인증·가스 공급자 장애가 함께 전파되지 않는 독립성을 별도로 확인한다. 승인된 M0 범위의 시험 준비이며 유료 인프라·실자산·운영 키 사용은 포함하지 않는다.
