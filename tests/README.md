# Testes automatizados

A suíte adiciona **17 cenários novos** sem modificar o código de produção ou a interface:

- `unit.rs`: 11 testes de regras de atalhos, conversão das teclas, serialização, histórico, estado de exportação e limites do desenho livre.
- `rendering_integration.rs`: 4 testes de integração entre documento, renderizador, PNG e sistema de arquivos. Verificam pixels das anotações, orientação, transparência, desfazer, arquivos inválidos e limpeza da captura temporária sem remover a exportação salva.
- `native_integration.rs`: 2 testes nativos explícitos. Um executa o binário real com `--diagnostics` e confere os 50 ciclos, o relatório, as imagens geradas e a limpeza dos arquivos. O outro usa registros reais do macOS para verificar suspensão, retomada, rejeição de rascunhos inválidos, rollback após conflito no segundo atalho e limpeza após falha de retomada.

Como o projeto tem somente um alvo binário, os testes importam os módulos reais com `#[path]`. Isso evita criar uma API pública ou alterar os arquivos em `src/`. Os testes unitários que já estão nesses módulos também são executados nos alvos importadores; portanto, a quantidade total de execuções inclui repetições dos testes existentes.

## Execução normal

```sh
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
```

Essa execução não abre o editor, não registra atalhos e não usa o clipboard geral. No Linux, somente os testes independentes do macOS são executados. A integração de renderização exige macOS, mas não precisa da permissão de Gravação de Tela.

## Integração nativa no macOS

Execute em uma sessão gráfica de usuário, fora de um sandbox que bloqueie AppKit, Carbon ou o serviço de clipboard:

```sh
cargo test --locked --test native_integration -- --ignored --test-threads=1 --nocapture
```

Esses dois testes ficam marcados como `ignored` apenas para evitar abertura de janelas e registro de atalhos na execução normal. O comando acima os executa e falha caso alguma verificação não passe.

O teste do editor usa conteúdo sintético e clipboard privado. O processo filho recebe uma pasta temporária exclusiva para o lock e as capturas, sem limpar a sessão de uma instância real do RXS. Há um limite de 120 segundos e encerramento do processo em caso de falha. Os resultados são verificados antes da remoção da pasta temporária.

O teste de atalhos procura quatro combinações livres com Command+Control+Option+Shift e letras. Não altera `NSUserDefaults`, atalhos do sistema nem preferências do RXS. Os registros são temporários e liberados ao final, inclusive durante o desenrolamento de uma falha do teste. Evite pressionar essas combinações durante a execução. Se não houver quatro combinações disponíveis, o teste falha com uma mensagem explícita.

## Integração contínua e limites

O workflow `tests.yml` executa os testes normais e formatação em Linux e macOS para pushes e pull requests. O Clippy com avisos tratados como erros roda no macOS, plataforma do aplicativo; no Linux os módulos nativos não são usados. Os testes nativos com janelas ficam para a execução explícita em uma sessão gráfica.

A suíte não comprova captura real com permissões do usuário, disparo por teclado físico, persistência de novas preferências após reiniciar nem interação com aplicativos de destino. Não simula resultados desses fluxos e não altera o código de produção para torná-los testáveis.
