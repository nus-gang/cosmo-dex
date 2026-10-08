# 네 검증인 command topology 준비

`chain_topology.prepare(python, candidate, nodes, fee_bps=0|25)`는 v0~v3 순서의 `{node_id, argv}` 네 항목을 받아 기존 `prepare_chain` 등록 packet 네 개를 생성한다. argv는 `chain_cli.py serve-chain-reviewed` 뒤 인자다. API 등록·socket·서비스 실행은 없다.

검사: 고유 node ID, RPC/P2P 8포트 중복 없음, 각 peer가 다른 세 node ID와 정확한 P2P 주소에 결합, 동일 bundle/artifact/input/profile/pin/승인 revision/수명, mutable home/scratch/mailbox/broker root의 상호 중첩 및 입력 경로 중첩 거절. 반환 packet은 입력 변경에 영향받지 않는다.

이는 command 원문 대조다. node ID와 private node_key, validator key/genesis, filesystem symlink/inode, fee0/25 전체 home 분리 및 실제 port availability 검사는 후속 초기화/topology 연결에서 필요하다. 조직 승인·기동 허가·DEV PASS가 아니다. 두 fee profile의 실서비스 동시 실행을 승인하지 않는다.

검증: `python3 -B -m unittest test_chain_topology test_chain_registration test_chain_cli -v` — 11 PASS/0 FAIL. 신규4, 기존7. 제어 API·실제 Chain/Rust/C 실행0. 전체 runtime manifest·fault driver·독립 승인·CTO→Security 심사는 남아 있다.
