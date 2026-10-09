# Validação do RXS 0.1.0

Data: 9 de setembro de 2026. Ambiente: macOS 26.6.2, Apple Silicon, compilação release com alvo mínimo macOS 13. Pacote gerado pelo script `scripts/bundle.sh`, com assinatura ad-hoc verificada.

## Resultado de memória

MB decimais. O indicador principal abaixo é **physical footprint**, a memória física atribuída ao processo pelo macOS. RSS inclui também páginas de bibliotecas compartilhadas e caches; os valores não são equivalentes.

| Situação | Physical footprint | RSS na mesma amostra |
|---|---:|---:|
| Início, sem editor | 10.50 MB | 68.68 MB |
| Maior amostra com editor 4K aberto | 40.42 MB | 216.65 MB |
| Maior amostra durante exportação 4K | 159.25 MB | 296.88 MB |
| Repouso após 50 ciclos e 2 segundos de estabilização | 37.39 MB | 678.45 MB |

As metas de 50 MB em repouso e 200 MB durante edição/exportação foram atendidas **em physical footprint nesta carga sintética**. Não foram atendidas se interpretadas como um teto de RSS. O mapa final mostra que a maior parte da memória residente corresponde a bibliotecas somente leitura compartilhadas do macOS. Não há promessa de teto universal: imagens diferentes, monitores, ferramentas do sistema e cargas de desenho podem mudar o resultado.

O diagnóstico cria uma imagem sintética 3840×2160 e faz 50 ciclos de abrir, desenhar, exportar e fechar. Aguarda a apresentação da janela e verifica depois de cada fechamento que não resta nenhuma área de desenho viva nem arquivo temporário de captura. A verificação final passou em todos os ciclos. A imagem e as anotações são liberadas explicitamente, independentemente de o AppKit reter uma janela fechada para animação/acessibilidade.

Dados: [amostras CSV](memory.csv), [mapa de memória final](vmmap-after-50.txt), [checagens nativas](native-checks.txt). As amostras não representam monitoramento contínuo de cada alocação. O processo temporário `screencapture`, WindowServer e serviços do clipboard não entram na medida do processo RXS. O diagnóstico usa uma área de transferência privada para não substituir o clipboard do usuário.

## Testes concluídos

- `cargo test --locked`: 6 testes passaram — coordenadas/zoom, geometria de setas, histórico limitado, desfazer/refazer, estado de exportação e validação de combinações de teclas.
- `cargo clippy --all-targets --locked -- -D warnings` e `cargo fmt --check`: passaram.
- `bash scripts/bundle.sh`, validação do Info.plist e `codesign --verify --strict`: passaram. O executável tem aproximadamente 674 KB e o pacote completo cerca de 800 KB em disco nesta compilação.
- Exportação nativa 4K: dimensões, orientação superior/inferior, transparência e posição das anotações verificadas por pixels.
- PNG codificado e decodificado após roundtrip no clipboard privado.
- Interface real: desenho de retângulo, desfazer/refazer, zoom 100%, ajuste à janela, rolagem, diálogo de salvar e salvamento de PNG na pasta de diagnóstico. O editor marcou a versão como exportada.
- Configuração de atalhos: conflito real de Command+Shift+3 com o macOS detectado, sem alterar as preferências do sistema.
- Fluxo de captura: seleção de região iniciada e cancelada com Escape; editor anterior restaurado. Tentativa de tela inteira negada pelo sistema, com erro compreensível e preservação do documento anterior.

## Pendências de validação no dispositivo

A permissão de Gravação de Tela não foi concedida durante o trabalho. Portanto, a captura efetiva de tela/região/janela ainda precisa ser conferida após a autorização pelo usuário. A versão final consulta e solicita essa permissão pelas APIs nativas antes de capturar, com acesso às configurações quando necessário; o diálogo de consentimento dessa última alteração não foi aprovado no teste.

Também permanecem pendentes: registro e disparo dos atalhos globais após liberar as combinações, testes em vários monitores, Intel, outras versões de macOS, colar no aplicativo de destino do usuário e uso prolongado com imagens reais. A geometria em pixels está coberta, mas isso não substitui uma rodada física com monitores de escalas diferentes.

A assinatura é local, não Developer ID. Não houve notarização, publicação ou instalação em /Applications. A janela demonstrativa utiliza apenas conteúdo sintético e não é uma captura real.

## Reprodução

Feche outra instância do RXS e execute a partir da raiz do projeto:

```sh
bash scripts/bundle.sh
dist/RXS.app/Contents/MacOS/rxs --diagnostics
```

Os novos resultados ficam em `target/diagnostics/`. O relatório presente registra a rodada final deste trabalho; não se atualiza automaticamente.

SHA-256 do executável verificado: `816f2530a0884d417d180790d5f0effea5dac101bef58f1af48f4ec77f88c366`.


## Ajustes do editor — 10 de setembro de 2026

Pacote `dist/RXS.app` recompilado e assinatura ad-hoc validada. Os 6 testes unitários, Clippy com avisos tratados como erros e formatação passaram.

Nova rodada do diagnóstico nativo passou: centralização no ajuste à janela e zooms de 10%, 25% e 100%, redimensionamento da área visível, abertura/fechamento dos controles RGB dentro do editor, atualização da cor e restauração da área de desenho. Verificadas também as políticas Regular com editor aberto e Accessory após fechar, além de janela configurada para permanecer visível ao desativar o app. Os 50 ciclos de exportação/fechamento passaram sem áreas de desenho ou capturas temporárias retidas. Relatório desta rodada em `target/diagnostics/native-checks.txt`.

O som nativo foi habilitado removendo a opção silenciosa de `screencapture`. A audição do som em uma captura real e a troca física via Command+Tab permanecem para validação manual. A ferramenta de inspeção visual não respondeu nesta rodada; as verificações acima foram executadas pelo diagnóstico nativo, com imagem sintética e clipboard privado.

## Color picker flutuante — 13/09/2026

Substituída a faixa RGB por um componente AppKit reutilizável com popover, HSB/RGB, HEX, opacidade, conta-gotas e coleção persistente. Os nove testes unitários, `cargo clippy --all-targets --locked -- -D warnings`, `cargo fmt --check` e o empacotamento assinado localmente passaram.

O diagnóstico nativo passou nos 50 ciclos, sem canvases ou capturas temporárias retidos. Valida abertura/fechamento sem alterar o viewport, callbacks RGBA, ações dos campos e sliders, rejeição de HEX e números inválidos, preservação de matiz acromática e transparência de uma anotação antes e depois do roundtrip PNG. A imagem `target/diagnostics/color-picker.png` foi inspecionada e as amostras da coleção corrigidas para renderizar cores explícitas.

A ferramenta de interação com a interface retornou timeout ao acessar o RXS. Permanecem para conferência manual: clique externo e cliques rápidos no botão, Escape e retorno de foco, percurso completo por Tab, seleção/cancelamento do conta-gotas, copiar para o clipboard geral, adição/reabertura da coleção e posicionamento nas bordas da tela. O diagnóstico não altera a coleção salva nem o clipboard geral.

### Ajuste da coleção de cores

Coleção reorganizada em cinco colunas, com até três linhas visíveis e rolagem vertical sobreposta. A view da coleção agora usa coordenadas a partir do topo, evitando deslocamento e corte das amostras. O botão de adicionar tem a mesma altura visual das amostras. Novas cores são trazidas para a área visível automaticamente. A prévia `target/diagnostics/color-picker-many.png` foi conferida com 23 cores sintéticas, sem modificar a coleção persistida do usuário.

## Cópia automática e gravação visual de atalhos — 20/09/2026

- 12 testes unitários passaram, incluindo conversão de códigos nativos, serialização dos atalhos, duplicidade, combinação reservada e apresentação dos modificadores. Clippy com avisos como erros e verificação de formatação passaram.
- Diagnóstico nativo com imagem sintética 4K: cópia automática em clipboard privado marca a versão como exportada; anotações voltam a exigir exportação; cópia manual compartilha o mesmo caminho e preserva dimensões, orientação e transparência. A rodada completa de 50 ciclos passou.
- Interface testada no pacote local: abrir Atalhos com ⌘ vírgula, gravar ⌘⌥3, rejeitar duplicidade na segunda ação, cancelar gravação com Escape e descartar o rascunho com Cancelar. ⌘S abriu o painel de salvar; cancelar manteve o editor. ⌘C apresentou “Imagem copiada”. As preferências de captura existentes foram preservadas.
- Aparências clara e escura inspecionadas em `target/diagnostics/shortcuts-light.png` e `shortcuts-dark.png`. As imagens usam RGBA de 8 bits para evitar incompatibilidades com bitmaps nativos HDR.
- Pacote local recompilado em `dist/RXS.app`, com Info.plist e assinatura ad-hoc verificados. Não foi instalado nem publicado.

Continuam pendentes nesta rodada: captura real com permissão de Gravação de Tela, colagem em outro aplicativo, aplicação/persistência de novos atalhos globais com reinício, suspensão/restauração de registros globais em uso e falha real de registro. Esses cenários não são comprovados pelos testes sintéticos ou pela gravação de um rascunho na janela.

## Componentes com aparência de aplicações web — 09/10/2026

O RSX agora usa componentes visuais próprios para botões, seletores, grupos de ferramentas, campos e cabeçalhos de tabela. O editor tem cabeçalho de exportação separado, ferramentas adaptáveis e área de trabalho com margens e borda. Atalhos, cores, logs e a janela Aparência compartilham os mesmos papéis de cor. A implementação mantém Rust, AppKit e Core Graphics, sem WebView.

Verificações executadas nesta rodada:

- Testes automatizados, Clippy com avisos tratados como erros e formatação passaram.
- Os dois testes nativos explícitos passaram: 50 ciclos de abrir/copiar/exportar/fechar com zero canvases ou capturas temporárias retidos, e suspensão/retomada/rollback de atalhos temporários.
- O diagnóstico aciona os botões reais de desfazer/refazer e o seletor de ferramentas, valida feedback de exportação, rejeita entradas de cor inválidas preservando o texto e verifica a seleção de logs após inserção e remoção de registros.
- Temas claro e escuro, prévia de aparência com cancelamento, editor padrão e compacto, seletor de cores, atalhos e logs foram renderizados e inspecionados. As prévias estão em `target/web-ui/`; o editor principal e as cores também foram atualizados em `docs/assets/`.
- Os tokens de texto comum, texto secundário, ação principal e erro são testados contra contraste mínimo de 4,5:1; o foco é testado contra o fundo com mínimo de 3:1. Isso não representa certificação de acessibilidade do app.
- O pacote local é gerado em `dist/RSX.app`, com assinatura ad-hoc; não foi instalado ou publicado.

O modo `--ui-diagnostics` permite repetir a verificação visual sem executar os 50 ciclos. Usa imagem sintética e clipboard privado, sem salvar temas, atalhos ou paletas. A execução exige acesso à sessão gráfica; o ambiente restrito pode impedir a resolução de aparências ou serviços de janela.

Continuam pendentes: captura real com permissões, monitores/Retina físicos, navegação completa por teclado físico, VoiceOver, persistência do tema após reiniciar e acompanhamento de uma alteração real do tema do sistema. O escuro/claro e a volta à herança do sistema foram verificados por diagnóstico; não foram realizados testes com usuários.

### Tabela de logs com componentes próprios — 09/10/2026

A tabela recebeu células desenhadas com espaçamento de 12 pontos, horário com dígitos de largura fixa, tipos em badges semânticos, separadores discretos, hover e seleção arredondada. O foco de teclado aparece na linha selecionada; a moldura nativa externa foi removida. Lista e detalhes usam superfícies arredondadas separadas por um divisor ajustável. Detalhes preservam seleção e cópia, com título destacado e identificadores em monospace.

Testes automatizados, formatação e Clippy passaram. O diagnóstico específico de logs passou com eventos sintéticos de seta para cima/baixo, atualização dos detalhes, filtros, preservação e fallback da seleção, redimensionamento e estados vazios/limitados. Renderizações clara e escura foram inspecionadas na janela mínima de 900 × 640 pontos, incluindo mensagem longa truncada na tabela e mensagem de erro quebrada nos detalhes. O contraste dos textos dos badges é verificado sobre suas superfícies compostas, inclusive em linhas selecionadas, com mínimo de 4,5:1.

As limitações manuais da rodada anterior continuam válidas, especialmente teclado físico, Retina e VoiceOver.

### Alinhamento da busca

O retângulo de desenho do campo agora usa a altura real da fonte para centralizar placeholder e conteúdo. A área de edição permanece centralizada. O diagnóstico de UI passou, e as renderizações do campo de busca vazio/preenchido e do seletor de cores foram inspecionadas.

## Assinatura estável e fluxo de permissão — 09/10/2026

Foi criada a identidade local RSX Local Code Signing no Chaves, com configuração pública fora do repositório e confiança limitada a assinatura de código. A compilação normal exige identidade disponível; CI/diagnósticos declaram ad-hoc explicitamente. A instalação prepara um candidato validado, confere o certificado configurado e a compatibilidade com a instalação anterior, respeita encerramento com alterações e mantém cópia de recuperação.

Os testes Rust de permissão passaram com autorização concedida, pedido inicial, recusa, reinício, revogação e ação explícita. A preferência salva registra apenas que um pedido já foi apresentado. Quatro testes do instalador passaram: identidade ausente, identidade incompatível, rollback após falha de validação e preservação da instalação quando mover a cópia anterior falha. Testes existentes, formatação e Clippy passaram.

A verificação nativa de dois executáveis distintos passou: ambos mantiveram o mesmo requisito de identidade, uma atualização compatível foi instalada em pasta temporária e versões incompatíveis/ad-hoc foram rejeitadas sem substituir o destino. A identidade foi reutilizada sem gerar novo certificado. O pacote assinado foi instalado em `/Applications/RSX.app`, com cópia anterior em `/Applications/.RSX-backup-20261009-175757-452926.app`; assinatura e igualdade do executável com o pacote foram verificadas. O aplicativo instalado abriu. A ferramenta de inspeção do RSX retornou timeout, mas a migração foi concluída pelos Ajustes com autorização específica do usuário: removida somente a entrada antiga do RSX e adicionada a cópia atual de `/Applications/RSX.app`. Desativar e reativar a entrada antiga não renovou sua identidade neste Mac. Depois da substituição, três diagnósticos reais passaram, cada um com duas capturas de 3420 × 2224 pixels e zero chamadas do app para solicitar permissão. A verificação incluiu instalar um pacote assinado com outro hash de executável (build de teste 4) e retornar ao pacote final (build 3), preservando a mesma identidade e a autorização em ambos. As imagens temporárias foram removidas, nenhum clipboard foi usado e os relatórios sem imagem estão em `target/permission-validation/`. O pacote final foi reaberto para uso normal. A migração da assinatura ad-hoc requer uma nova autorização de Gravação de Tela; não houve edição direta do banco nem reset global de permissões do macOS. A troca da entrada do RSX pela interface dos Ajustes foi autorizada separadamente pelo usuário.
