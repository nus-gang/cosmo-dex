# 새 home 이전 C 의미 검증 명령

`runtime/bootstrap_check.rs`는 전용 `validate-captured --capture-sha256 HASH --runtime-pin PIN --local-demo-profile ABS_PATH --acknowledge-unproven-space` 명령이다. stdin의 유한 capture 원문(최대48MiB)을 SHA/EOF로 결합하고 승인 C `Validated::decode_bundle`로 검증한다. home·키 디렉터리 인자는 받지 않으며 Engine/signer/RPC를 생성하지 않는다. 기존 worker 검증기는 기존 home을 여는 의미를 그대로 유지한다. 두 실행 파일은 `capture_io.rs`의 기존 bounded 파일/전송 검사를 공유한다.

성공 report의 semantic_validation=true는 C 입력 의미 검증만 뜻한다. approval_verified=false/service_started=false/durable_ack=false. 오류는 exit2와 고정 LOCAL_BOOTSTRAP_CHECK_REJECTED이며 입력/경로/키를 출력하지 않는다. standalone stdin이 정상 인자에서 EOF 없이 열려 있으면 자체 deadline은 없으므로 기존 bounded process supervisor가 유한 capture와 시간 상한을 제공해야 한다. 아직 descriptor/private 사본/인증 승인 reader·bootstrap create 최종 CLI에 연결하지 않았다. 실행 pin 발급이나 초기화 허가가 아니다.

검증: 기존 Rust1.92.0/cache로 실행 파일·시험·startup 컴파일 PASS. 신규3+공통 transport/startup 회귀3=6 PASS/0 FAIL/SKIP0. fee0/25 실제 C 검증·subprocess 고정 report·home 생성0, capture 절단·guard 변조·profile 링크, 두 opt-in/중복/미지원 인자/잘못된 hash 거절·열린 stdin 즉시 종료. 합성 descriptor/pin/등록키 사용, 실제 조직 승인·서비스·RPC0. 기존 startup 포함301 중298개 미실행. 최초 fixture 필드명을 profile로 잘못 쓴 컴파일 실패를 effective_profile로 수정했고 원 로그 보존.

다음 SRE 작업은 초기화 checker의 descriptor/인증 경로 및 create CLI 연결, chain 관리 launcher/fault/정리, 최종 build/다섯 descriptor manifest, CEO/CTO 독립 승인·CTO→Security 심사다. DEV NOT_RUN·runtime pin 미발급·€0. G00=FAIL_UNPROVEN / allowlist=[] / ACK=CLOSED / durable_ack=false 유지.
