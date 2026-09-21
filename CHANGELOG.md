# Changelog

## 0.3.0

- Solicitação de Gravação de Tela limitada a uma vez por execução, evitando pedidos nativos repetidos ao usar o atalho.
- Fluxo de autorização mais claro: o RXS abre os Ajustes e encerra para que o macOS possa aplicar a nova permissão.
- Orientação para renovar entradas antigas do RXS quando uma atualização assinada localmente deixa uma autorização obsoleta.
- Suporte opcional a assinatura Developer ID pelo `RXS_CODESIGN_IDENTITY`, preservando a identidade do app entre versões.
- Aviso explícito ao gerar builds ad-hoc, cujas permissões de privacidade podem precisar ser concedidas novamente após atualizações.

## 0.2.0

- Capturas copiadas automaticamente para a área de transferência antes de abrir o editor.
- Ações Copiar ⌘C e Salvar ⌘S visíveis no editor, com feedback de cópia.
- Janela nativa para gravar combinações de teclas, detectar conflitos e cancelar alterações.
- Suspensão temporária dos atalhos durante a gravação e recuperação após falhas de registro.
- Testes unitários e de integração para edição, atalhos, PNG, arquivos e serviços nativos do macOS.
- Verificação automática em pushes e pull requests; distribuição para Apple Silicon e Intel por tags de versão.

## 0.1.0

- Primeira versão do editor nativo de capturas e anotações para macOS.
