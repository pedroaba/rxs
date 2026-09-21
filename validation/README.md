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
